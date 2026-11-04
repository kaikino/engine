//! The picture processing unit.
//!
//! | Spec                | Value                                           |
//! |---------------------|-------------------------------------------------|
//! | Resolution          | 256 × 224, 60 Hz                                |
//! | Colors              | fixed 32-color palette ([`palette`])            |
//! | Tile memory         | 512 tiles of 8×8, arranged as a 16-tile-wide sheet |
//! | Backgrounds         | 2 layers, 32×32 tiles each, wrap-around scroll  |
//! | Sprites             | 128 per frame, 16 per scanline, 1×1 to 4×4 tiles |
//! | Overlay             | direct pixel/line/rect/text drawing on top; ASCII + katakana font |
//! | Color key           | one palette index is transparent in tiles (default 0) |
//! | Color remap         | 32-entry remap table applied at scan-out (palette swaps, flashes) |
//! | Raster effects      | per-scanline scroll per layer, horizontal window masks, mosaic |
//!
//! A frame is built in `draw`: set scroll, write tilemaps, push sprites, draw
//! overlay. The runtime then calls [`Gpu::render`], which composites
//! backdrop → BG0 → BG1 → sprites → priority tiles → overlay into the
//! framebuffer, applies the remap table, and converts to RGBA for display.
//!
//! The 16-sprites-per-scanline limit is real: sprites past the limit on a
//! given line are not drawn on that line, exactly like 1990 hardware. Games
//! can read [`Gpu::dropped_sprite_lines`] to tune around it (or flicker on
//! purpose).

pub mod art;
pub mod font;
pub mod kana;
pub mod palette;
pub mod png;

/// Screen width in pixels.
pub const SCREEN_W: usize = 256;
/// Screen height in pixels.
pub const SCREEN_H: usize = 224;
/// Pixels per tile edge.
pub const TILE_SIZE: usize = 8;
/// Pixels per tile.
pub const TILE_PIXELS: usize = TILE_SIZE * TILE_SIZE;
/// Tiles in tile memory.
pub const TILE_COUNT: usize = 512;
/// Width of the conceptual tile sheet in tiles. Multi-tile sprites take their
/// second row of tiles from `tile + SHEET_W`.
pub const SHEET_W: usize = 16;
/// Background map width and height in tiles.
pub const MAP_SIZE: usize = 32;
/// Number of background layers.
pub const BG_LAYERS: usize = 2;
/// Maximum sprites per frame.
pub const MAX_SPRITES: usize = 128;
/// Maximum sprites drawn on one scanline.
pub const SPRITES_PER_SCANLINE: usize = 16;
/// Overlay value meaning "no pixel".
const OVERLAY_EMPTY: u8 = 0xFF;

/// Tilemap entry bit: flip horizontally.
pub const FLIP_X: u16 = 1 << 14;
/// Tilemap entry bit: flip vertically.
pub const FLIP_Y: u16 = 1 << 15;
/// Tilemap entry bit: draw this tile above sprites.
pub const PRIORITY: u16 = 1 << 13;
const TILE_MASK: u16 = 0x1FF;

/// One sprite in object memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Sprite {
    /// Left edge in screen pixels (may be off-screen).
    pub x: i32,
    /// Top edge in screen pixels (may be off-screen).
    pub y: i32,
    /// Top-left tile index in tile memory.
    pub tile: u16,
    /// Width in tiles (clamped to `1..=4`).
    pub w: u8,
    /// Height in tiles (clamped to `1..=4`).
    pub h: u8,
    /// Mirror horizontally.
    pub flip_x: bool,
    /// Mirror vertically.
    pub flip_y: bool,
}

impl Sprite {
    /// A 1×1 sprite at a position.
    pub const fn new(x: i32, y: i32, tile: u16) -> Sprite {
        Sprite {
            x,
            y,
            tile,
            w: 1,
            h: 1,
            flip_x: false,
            flip_y: false,
        }
    }
    /// Sets the size in tiles.
    pub const fn size(mut self, w: u8, h: u8) -> Sprite {
        self.w = w;
        self.h = h;
        self
    }
    /// Sets horizontal flip.
    pub const fn flip_x(mut self, f: bool) -> Sprite {
        self.flip_x = f;
        self
    }
    /// Sets vertical flip.
    pub const fn flip_y(mut self, f: bool) -> Sprite {
        self.flip_y = f;
        self
    }
    fn width_px(&self) -> i32 {
        i32::from(self.w.clamp(1, 4)) * TILE_SIZE as i32
    }
    fn height_px(&self) -> i32 {
        i32::from(self.h.clamp(1, 4)) * TILE_SIZE as i32
    }
}

/// A horizontal window: the layer is drawn only inside (or only outside)
/// the pixel columns `x0..x1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// First visible column (inclusive).
    pub x0: i32,
    /// End column (exclusive).
    pub x1: i32,
    /// `true`: draw inside the window. `false`: draw outside it.
    pub inside: bool,
}

impl Window {
    /// Builds a window.
    pub const fn new(x0: i32, x1: i32, inside: bool) -> Window {
        Window { x0, x1, inside }
    }
    #[inline]
    fn allows(&self, x: i32) -> bool {
        (x >= self.x0 && x < self.x1) == self.inside
    }
}

/// A background layer.
#[derive(Clone)]
pub struct Background {
    map: Vec<u16>,
    /// Horizontal scroll in pixels (wraps every 256).
    pub scroll_x: i32,
    /// Vertical scroll in pixels (wraps every 256).
    pub scroll_y: i32,
    /// Whether the layer is drawn.
    pub enabled: bool,
    /// Per-scanline horizontal scroll added to `scroll_x`, one entry per
    /// screen line. Empty means off. Set it with [`Background::set_line_scroll`].
    pub line_scroll_x: Vec<i32>,
    /// Optional horizontal window mask.
    pub window: Option<Window>,
}

impl Default for Background {
    fn default() -> Self {
        Background {
            map: vec![0; MAP_SIZE * MAP_SIZE],
            scroll_x: 0,
            scroll_y: 0,
            enabled: false,
            line_scroll_x: Vec::new(),
            window: None,
        }
    }
}

impl Background {
    /// Fills the per-scanline scroll table from a function of the line
    /// number (the "HDMA" effect: waves, heat haze, parallax strips).
    pub fn set_line_scroll(&mut self, f: impl Fn(usize) -> i32) {
        self.line_scroll_x = (0..SCREEN_H).map(f).collect();
    }

    /// Disables per-scanline scrolling.
    pub fn clear_line_scroll(&mut self) {
        self.line_scroll_x.clear();
    }

    #[inline]
    fn scroll_x_at(&self, y: usize) -> i32 {
        self.scroll_x + self.line_scroll_x.get(y).copied().unwrap_or(0)
    }

    /// Writes a tilemap entry (tile index plus flag bits). Coordinates wrap.
    pub fn set(&mut self, tx: i32, ty: i32, entry: u16) {
        let x = tx.rem_euclid(MAP_SIZE as i32) as usize;
        let y = ty.rem_euclid(MAP_SIZE as i32) as usize;
        self.map[y * MAP_SIZE + x] = entry;
    }
    /// Reads a tilemap entry. Coordinates wrap.
    pub fn get(&self, tx: i32, ty: i32) -> u16 {
        let x = tx.rem_euclid(MAP_SIZE as i32) as usize;
        let y = ty.rem_euclid(MAP_SIZE as i32) as usize;
        self.map[y * MAP_SIZE + x]
    }
    /// Fills the whole map with one entry.
    pub fn fill(&mut self, entry: u16) {
        self.map.iter_mut().for_each(|e| *e = entry);
    }
    /// Fills a rectangle of tiles.
    pub fn fill_rect(&mut self, tx: i32, ty: i32, w: i32, h: i32, entry: u16) {
        for y in ty..ty + h {
            for x in tx..tx + w {
                self.set(x, y, entry);
            }
        }
    }
}

/// The picture processor. One per console.
#[derive(Clone)]
pub struct Gpu {
    tiles: Vec<u8>,
    /// The two background layers. `bg[0]` is drawn first (behind `bg[1]`).
    pub bg: [Background; BG_LAYERS],
    oam: Vec<Sprite>,
    overlay: Vec<u8>,
    frame: Vec<u8>,
    rgba: Vec<u8>,
    remap: [u8; palette::PALETTE_SIZE],
    backdrop: u8,
    color_key: u8,
    mosaic: u8,
    dropped: u32,
    sprites_rejected: u32,
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Gpu {
    /// Creates a GPU with empty tile memory and a black screen.
    pub fn new() -> Gpu {
        let mut remap = [0u8; palette::PALETTE_SIZE];
        for (i, r) in remap.iter_mut().enumerate() {
            *r = i as u8;
        }
        Gpu {
            tiles: vec![0; TILE_COUNT * TILE_PIXELS],
            bg: [Background::default(), Background::default()],
            oam: Vec::with_capacity(MAX_SPRITES),
            overlay: vec![OVERLAY_EMPTY; SCREEN_W * SCREEN_H],
            frame: vec![0; SCREEN_W * SCREEN_H],
            rgba: vec![0; SCREEN_W * SCREEN_H * 4],
            remap,
            backdrop: palette::SUMI,
            color_key: palette::SUMI,
            mosaic: 1,
            dropped: 0,
            sprites_rejected: 0,
        }
    }

    /// Sets the mosaic block size (`1` = off, up to `16`): backgrounds and
    /// sprites are pixelated into blocks at scan-out. The overlay is not
    /// affected. The classic transition and hit effect.
    pub fn set_mosaic(&mut self, size: u8) {
        self.mosaic = size.clamp(1, 16);
    }

    /// Current mosaic block size.
    pub fn mosaic(&self) -> u8 {
        self.mosaic
    }

    // ----- frame lifecycle -------------------------------------------------

    /// Clears per-frame state: object memory and the overlay. The runtime calls
    /// this before every `draw`; headless tests call it themselves.
    pub fn begin_frame(&mut self) {
        self.oam.clear();
        self.overlay.iter_mut().for_each(|p| *p = OVERLAY_EMPTY);
        self.dropped = 0;
        self.sprites_rejected = 0;
    }

    /// Sets the backdrop color (what shows where nothing is drawn).
    pub fn clear(&mut self, color: u8) {
        self.backdrop = color % palette::PALETTE_SIZE as u8;
    }

    /// Current backdrop color.
    pub fn backdrop(&self) -> u8 {
        self.backdrop
    }

    /// Sets which palette index is transparent in tiles and sprites.
    pub fn set_color_key(&mut self, color: u8) {
        self.color_key = color % palette::PALETTE_SIZE as u8;
    }

    /// Current color key.
    pub fn color_key(&self) -> u8 {
        self.color_key
    }

    /// Remaps a palette index at scan-out: every pixel of `from` displays as
    /// `to`. Use for palette swaps, hit flashes and fades.
    pub fn pal(&mut self, from: u8, to: u8) {
        self.remap[(from as usize) % palette::PALETTE_SIZE] = to % palette::PALETTE_SIZE as u8;
    }

    /// Resets the remap table to identity.
    pub fn pal_reset(&mut self) {
        for (i, r) in self.remap.iter_mut().enumerate() {
            *r = i as u8;
        }
    }

    // ----- tile memory ----------------------------------------------------

    /// Copies tiles into tile memory starting at `index`. `data` holds 64
    /// bytes per tile; anything past the end of tile memory is ignored.
    pub fn load_tiles(&mut self, index: usize, data: &[u8]) {
        let start = (index * TILE_PIXELS).min(self.tiles.len());
        let end = (start + data.len()).min(self.tiles.len());
        self.tiles[start..end].copy_from_slice(&data[..end - start]);
    }

    /// Reads one pixel of a tile (no flipping).
    pub fn tile_pixel(&self, tile: u16, px: usize, py: usize) -> u8 {
        let t = (tile as usize) % TILE_COUNT;
        self.tiles[t * TILE_PIXELS + (py % TILE_SIZE) * TILE_SIZE + (px % TILE_SIZE)]
    }

    // ----- sprites ---------------------------------------------------------

    /// Pushes a sprite. Lower indices draw on top. Returns `false` (and drops
    /// the sprite) once 128 sprites have been pushed this frame.
    pub fn sprite(&mut self, s: Sprite) -> bool {
        if self.oam.len() >= MAX_SPRITES {
            self.sprites_rejected += 1;
            return false;
        }
        self.oam.push(s);
        true
    }

    /// Number of sprites pushed this frame.
    pub fn sprite_count(&self) -> usize {
        self.oam.len()
    }

    /// Sprites rejected this frame because object memory was full.
    pub fn sprites_rejected(&self) -> u32 {
        self.sprites_rejected
    }

    /// Number of (sprite, scanline) pairs skipped by the per-scanline limit
    /// in the last [`Gpu::render`].
    pub fn dropped_sprite_lines(&self) -> u32 {
        self.dropped
    }

    // ----- overlay drawing -------------------------------------------------

    /// Sets one overlay pixel. Off-screen coordinates are ignored.
    #[inline]
    pub fn pixel(&mut self, x: i32, y: i32, color: u8) {
        if x >= 0 && y >= 0 && (x as usize) < SCREEN_W && (y as usize) < SCREEN_H {
            self.overlay[y as usize * SCREEN_W + x as usize] = color % palette::PALETTE_SIZE as u8;
        }
    }

    /// Fills a rectangle on the overlay.
    pub fn rect_fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(SCREEN_W as i32);
        let y1 = (y + h).min(SCREEN_H as i32);
        let c = color % palette::PALETTE_SIZE as u8;
        for yy in y0..y1 {
            let row = yy as usize * SCREEN_W;
            for xx in x0..x1 {
                self.overlay[row + xx as usize] = c;
            }
        }
    }

    /// Outlines a rectangle on the overlay.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.line(x, y, x + w - 1, y, color);
        self.line(x, y + h - 1, x + w - 1, y + h - 1, color);
        self.line(x, y, x, y + h - 1, color);
        self.line(x + w - 1, y, x + w - 1, y + h - 1, color);
    }

    /// Draws a line on the overlay (Bresenham).
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.pixel(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Draws text with the system font on the overlay: printable ASCII and
    /// katakana (voiced marks composed). Anything else draws as `?`. Returns
    /// the x coordinate after the text.
    pub fn text(&mut self, x: i32, y: i32, s: &str, color: u8) -> i32 {
        let mut cx = x;
        for ch in s.chars() {
            if ch == '\n' {
                continue;
            }
            let g = font::glyph_any(ch);
            for (row, bits) in g.iter().enumerate() {
                if *bits == 0 {
                    continue;
                }
                for col in 0..8 {
                    if bits & (0x80 >> col) != 0 {
                        self.pixel(cx + col, y + row as i32, color);
                    }
                }
            }
            cx += font::GLYPH_W;
        }
        cx
    }

    /// Draws text centered horizontally on the screen.
    pub fn text_centered(&mut self, y: i32, s: &str, color: u8) {
        let w = s.chars().count() as i32 * font::GLYPH_W;
        self.text((SCREEN_W as i32 - w) / 2, y, s, color);
    }

    /// Draws a single tile directly on the overlay (color-keyed), bypassing
    /// object memory and its limits. Handy for HUD icons.
    pub fn blit_tile(&mut self, x: i32, y: i32, tile: u16, flip_x: bool, flip_y: bool) {
        for py in 0..TILE_SIZE {
            for px in 0..TILE_SIZE {
                let sx = if flip_x { TILE_SIZE - 1 - px } else { px };
                let sy = if flip_y { TILE_SIZE - 1 - py } else { py };
                let c = self.tile_pixel(tile, sx, sy);
                if c != self.color_key {
                    self.pixel(x + px as i32, y + py as i32, c);
                }
            }
        }
    }

    // ----- scan-out --------------------------------------------------------

    /// Composites the frame. Call once per displayed frame after `draw`.
    pub fn render(&mut self) {
        self.dropped = 0;
        let key = self.color_key;
        let mut line_sprites: Vec<usize> = Vec::with_capacity(SPRITES_PER_SCANLINE);
        for y in 0..SCREEN_H {
            let row = y * SCREEN_W;
            self.frame[row..row + SCREEN_W]
                .iter_mut()
                .for_each(|p| *p = self.backdrop);
            // Backgrounds, low priority.
            for layer in 0..BG_LAYERS {
                self.render_bg_line(layer, y, false);
            }
            // Sprites: select up to 16 for this line, in OAM order.
            line_sprites.clear();
            let yi = y as i32;
            for (i, s) in self.oam.iter().enumerate() {
                if yi >= s.y && yi < s.y + s.height_px() {
                    if line_sprites.len() < SPRITES_PER_SCANLINE {
                        line_sprites.push(i);
                    } else {
                        self.dropped += 1;
                    }
                }
            }
            // Draw back-to-front so lower OAM indices end up on top.
            for &i in line_sprites.iter().rev() {
                let s = self.oam[i];
                let w = s.width_px();
                let h = s.height_px();
                let local_y = yi - s.y;
                let ty = if s.flip_y { h - 1 - local_y } else { local_y };
                let tile_row = (ty / TILE_SIZE as i32) as usize;
                let py = (ty % TILE_SIZE as i32) as usize;
                for lx in 0..w {
                    let x = s.x + lx;
                    if x < 0 || x >= SCREEN_W as i32 {
                        continue;
                    }
                    let tx = if s.flip_x { w - 1 - lx } else { lx };
                    let tile_col = (tx / TILE_SIZE as i32) as usize;
                    let px = (tx % TILE_SIZE as i32) as usize;
                    let tile = (s.tile as usize + tile_row * SHEET_W + tile_col) % TILE_COUNT;
                    let c = self.tiles[tile * TILE_PIXELS + py * TILE_SIZE + px];
                    if c != key {
                        self.frame[row + x as usize] = c;
                    }
                }
            }
            // Backgrounds, high priority tiles over sprites.
            for layer in 0..BG_LAYERS {
                self.render_bg_line(layer, y, true);
            }
        }
        // Mosaic: every block shows its top-left pixel.
        if self.mosaic > 1 {
            let m = usize::from(self.mosaic);
            for y in 0..SCREEN_H {
                let sy = y - y % m;
                for x in 0..SCREEN_W {
                    let sx = x - x % m;
                    self.frame[y * SCREEN_W + x] = self.frame[sy * SCREEN_W + sx];
                }
            }
        }
        // Overlay, above everything and unaffected by mosaic.
        for (dst, &o) in self.frame.iter_mut().zip(self.overlay.iter()) {
            if o != OVERLAY_EMPTY {
                *dst = o;
            }
        }
        // Remap and convert.
        for (i, &c) in self.frame.iter().enumerate() {
            let [r, g, b] = palette::rgb(self.remap[c as usize % palette::PALETTE_SIZE]);
            self.rgba[i * 4] = r;
            self.rgba[i * 4 + 1] = g;
            self.rgba[i * 4 + 2] = b;
            self.rgba[i * 4 + 3] = 0xFF;
        }
    }

    fn render_bg_line(&mut self, layer: usize, y: usize, priority_pass: bool) {
        let bg = &self.bg[layer];
        if !bg.enabled {
            return;
        }
        let key = self.color_key;
        let row = y * SCREEN_W;
        let wy = (y as i32 + bg.scroll_y).rem_euclid((MAP_SIZE * TILE_SIZE) as i32) as usize;
        let ty = wy / TILE_SIZE;
        let py = wy % TILE_SIZE;
        let scroll_x = bg.scroll_x_at(y);
        let window = bg.window;
        for x in 0..SCREEN_W {
            if let Some(w) = window {
                if !w.allows(x as i32) {
                    continue;
                }
            }
            let wx = (x as i32 + scroll_x).rem_euclid((MAP_SIZE * TILE_SIZE) as i32) as usize;
            let entry = bg.map[ty * MAP_SIZE + wx / TILE_SIZE];
            if (entry & PRIORITY != 0) != priority_pass {
                continue;
            }
            let tile = (entry & TILE_MASK) as usize % TILE_COUNT;
            let mut px = wx % TILE_SIZE;
            let mut ppy = py;
            if entry & FLIP_X != 0 {
                px = TILE_SIZE - 1 - px;
            }
            if entry & FLIP_Y != 0 {
                ppy = TILE_SIZE - 1 - ppy;
            }
            let c = self.tiles[tile * TILE_PIXELS + ppy * TILE_SIZE + px];
            if c != key {
                self.frame[row + x] = c;
            }
        }
    }

    /// The composited frame as palette indices (before remap), row-major.
    pub fn frame(&self) -> &[u8] {
        &self.frame
    }

    /// The composited frame as RGBA8 after remap, row-major, 256×224×4 bytes.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Reads a composited pixel's palette index (before remap).
    pub fn get_pixel(&self, x: usize, y: usize) -> u8 {
        self.frame[(y % SCREEN_H) * SCREEN_W + (x % SCREEN_W)]
    }

    /// Encodes the last rendered frame as a PNG.
    pub fn screenshot_png(&self) -> Vec<u8> {
        png::encode_rgba(SCREEN_W as u32, SCREEN_H as u32, &self.rgba)
    }

    /// 64-bit hash of the last rendered frame; useful for golden tests.
    pub fn frame_hash(&self) -> u64 {
        let mut h = crate::hash::StateHasher::new();
        h.write_bytes(&self.frame);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_tile(c: u8) -> Vec<u8> {
        vec![c; TILE_PIXELS]
    }

    #[test]
    fn backdrop_and_clear() {
        let mut g = Gpu::new();
        g.clear(palette::AI);
        g.render();
        assert!(g.frame().iter().all(|&p| p == palette::AI));
        assert_eq!(g.backdrop(), palette::AI);
        let rgba = g.rgba();
        assert_eq!(&rgba[..4], &[0x16, 0x5e, 0x83, 0xff]);
        assert_eq!(Gpu::default().backdrop(), palette::SUMI);
    }

    #[test]
    fn background_layers_scroll_and_flip() {
        let mut g = Gpu::new();
        let mut tile = vec![palette::SUMI; TILE_PIXELS];
        tile[0] = palette::BENI; // top-left pixel only
        g.load_tiles(1, &tile);
        g.bg[0].enabled = true;
        g.bg[0].set(0, 0, 1);
        g.render();
        assert_eq!(g.get_pixel(0, 0), palette::BENI);
        assert_eq!(g.get_pixel(1, 0), palette::SUMI);
        g.bg[0].set(0, 0, 1 | FLIP_X);
        g.render();
        assert_eq!(g.get_pixel(7, 0), palette::BENI);
        g.bg[0].set(0, 0, 1 | FLIP_Y);
        g.render();
        assert_eq!(g.get_pixel(0, 7), palette::BENI);
        g.bg[0].set(0, 0, 1);
        g.bg[0].scroll_x = 1;
        g.bg[0].scroll_y = 0;
        g.render();
        // Scrolling right by 1 moves the pixel left off-screen and wraps.
        assert_eq!(g.get_pixel(255, 0), palette::BENI);
        g.bg[0].scroll_x = -3;
        g.render();
        assert_eq!(g.get_pixel(3, 0), palette::BENI);
        assert_eq!(g.bg[0].get(32, 32), 1);
        g.bg[0].fill(0);
        assert_eq!(g.bg[0].get(0, 0), 0);
        g.bg[0].fill_rect(1, 1, 2, 2, 5);
        assert_eq!(g.bg[0].get(2, 2), 5);
        assert_eq!(g.bg[0].get(3, 3), 0);
        // Layer 1 draws over layer 0.
        g.bg[0].fill(0);
        g.load_tiles(2, &solid_tile(palette::KI));
        g.load_tiles(3, &solid_tile(palette::SORA));
        g.bg[0].scroll_x = 0;
        g.bg[0].fill(2);
        g.bg[1].enabled = true;
        g.bg[1].fill(3);
        g.render();
        assert_eq!(g.get_pixel(100, 100), palette::SORA);
        g.bg[1].enabled = false;
        g.render();
        assert_eq!(g.get_pixel(100, 100), palette::KI);
    }

    #[test]
    fn sprites_draw_with_priority_and_flip() {
        let mut g = Gpu::new();
        let mut tile = vec![palette::SUMI; TILE_PIXELS];
        tile[7] = palette::SHU; // top-right pixel
        g.load_tiles(10, &tile);
        g.load_tiles(11, &solid_tile(palette::KI));
        g.begin_frame();
        assert!(g.sprite(Sprite::new(10, 20, 10)));
        g.render();
        assert_eq!(g.get_pixel(17, 20), palette::SHU);
        assert_eq!(g.get_pixel(10, 20), palette::SUMI);
        g.begin_frame();
        g.sprite(Sprite::new(10, 20, 10).flip_x(true));
        g.render();
        assert_eq!(g.get_pixel(10, 20), palette::SHU);
        g.begin_frame();
        g.sprite(Sprite::new(10, 20, 10).flip_y(true));
        g.render();
        assert_eq!(g.get_pixel(17, 27), palette::SHU);
        // Lower OAM index is on top.
        g.begin_frame();
        g.sprite(Sprite::new(10, 20, 10));
        g.sprite(Sprite::new(10, 20, 11));
        g.render();
        assert_eq!(g.get_pixel(17, 20), palette::SHU);
        assert_eq!(g.get_pixel(10, 20), palette::KI);
        // Partially off-screen sprites clip instead of wrapping.
        g.begin_frame();
        g.sprite(Sprite::new(-4, -4, 11));
        g.sprite(Sprite::new(252, 220, 11));
        g.render();
        assert_eq!(g.get_pixel(0, 0), palette::KI);
        assert_eq!(g.get_pixel(255, 223), palette::KI);
        assert_eq!(g.get_pixel(4, 4), palette::SUMI);
    }

    #[test]
    fn multi_tile_sprites_use_sheet_layout() {
        let mut g = Gpu::new();
        g.load_tiles(0, &solid_tile(1));
        g.load_tiles(1, &solid_tile(2));
        g.load_tiles(SHEET_W, &solid_tile(3));
        g.load_tiles(SHEET_W + 1, &solid_tile(4));
        g.begin_frame();
        g.sprite(Sprite::new(0, 0, 0).size(2, 2));
        g.render();
        assert_eq!(g.get_pixel(0, 0), 1);
        assert_eq!(g.get_pixel(8, 0), 2);
        assert_eq!(g.get_pixel(0, 8), 3);
        assert_eq!(g.get_pixel(8, 8), 4);
        g.begin_frame();
        g.sprite(Sprite::new(0, 0, 0).size(2, 2).flip_x(true).flip_y(true));
        g.render();
        assert_eq!(g.get_pixel(0, 0), 4);
        assert_eq!(g.get_pixel(15, 15), 1);
        // Size is clamped to 1..=4.
        g.begin_frame();
        g.sprite(Sprite::new(0, 0, 0).size(0, 9));
        g.render();
        // h clamps to 4 tiles: rows 1..3 read tiles 16, 32, 48; only 16 is set.
        assert_eq!(g.get_pixel(0, 8), 3);
        assert_eq!(g.get_pixel(0, 31), palette::SUMI);
        assert_eq!(g.get_pixel(8, 0), palette::SUMI);
    }

    #[test]
    fn scanline_limit_drops_sprites() {
        let mut g = Gpu::new();
        g.load_tiles(0, &solid_tile(palette::KI));
        g.begin_frame();
        for i in 0..20 {
            g.sprite(Sprite::new(i * 8, 0, 0));
        }
        g.render();
        // The first 16 draw, the last 4 are dropped on all 8 lines.
        assert_eq!(g.get_pixel(15 * 8, 0), palette::KI);
        assert_eq!(g.get_pixel(16 * 8, 0), palette::SUMI);
        assert_eq!(g.dropped_sprite_lines(), 4 * 8);
        // OAM capacity.
        g.begin_frame();
        for i in 0..130 {
            g.sprite(Sprite::new(0, i, 0));
        }
        assert_eq!(g.sprite_count(), MAX_SPRITES);
        assert_eq!(g.sprites_rejected(), 2);
    }

    #[test]
    fn priority_tiles_cover_sprites() {
        let mut g = Gpu::new();
        g.load_tiles(0, &solid_tile(palette::KI));
        g.load_tiles(1, &solid_tile(palette::KI));
        g.load_tiles(2, &solid_tile(palette::SORA));
        g.bg[0].enabled = true;
        g.bg[0].set(0, 0, 2 | PRIORITY);
        g.bg[0].set(1, 0, 2);
        g.begin_frame();
        g.sprite(Sprite::new(0, 0, 0).size(2, 1));
        g.render();
        assert_eq!(g.get_pixel(0, 0), palette::SORA);
        assert_eq!(g.get_pixel(8, 0), palette::KI);
    }

    #[test]
    fn overlay_primitives_and_text() {
        let mut g = Gpu::new();
        g.begin_frame();
        g.pixel(5, 5, palette::GOFUN);
        g.pixel(-1, 5, palette::GOFUN);
        g.pixel(5, 9999, palette::GOFUN);
        g.rect_fill(10, 10, 4, 4, palette::BENI);
        g.rect(20, 20, 5, 5, palette::KI);
        g.rect(20, 20, 0, 5, palette::KI);
        g.line(0, 100, 50, 120, palette::AI);
        g.line(50, 120, 0, 100, palette::AI);
        g.line(30, 30, 30, 60, palette::AI);
        let end = g.text(0, 200, "AB\n", palette::GOFUN);
        assert_eq!(end, 16);
        g.text_centered(210, "KAGAMI", palette::GOFUN);
        g.render();
        assert_eq!(g.get_pixel(5, 5), palette::GOFUN);
        assert_eq!(g.get_pixel(11, 11), palette::BENI);
        assert_eq!(g.get_pixel(14, 10), palette::SUMI);
        assert_eq!(g.get_pixel(20, 20), palette::KI);
        assert_eq!(g.get_pixel(24, 24), palette::KI);
        assert_eq!(g.get_pixel(22, 22), palette::SUMI);
        assert_eq!(g.get_pixel(0, 100), palette::AI);
        assert_eq!(g.get_pixel(50, 120), palette::AI);
        assert_eq!(g.get_pixel(30, 45), palette::AI);
        // 'A' has its apex at column 2 of row 0.
        assert_eq!(g.get_pixel(2, 200), palette::GOFUN);
        // Centered text starts at (256 - 48) / 2 = 104.
        assert_eq!(g.get_pixel(104, 211), palette::GOFUN); // 'K' left stem
                                                           // Overlay can draw the key color (index 0) explicitly.
        g.clear(palette::GOFUN);
        g.begin_frame();
        g.pixel(1, 1, palette::SUMI);
        g.render();
        assert_eq!(g.get_pixel(1, 1), palette::SUMI);
        assert_eq!(g.get_pixel(2, 2), palette::GOFUN);
    }

    #[test]
    fn katakana_text_draws_and_composes_marks() {
        let mut g = Gpu::new();
        g.begin_frame();
        g.text(0, 180, "カ", palette::GOFUN);
        g.text(8, 180, "ガ", palette::GOFUN);
        g.text(16, 180, "あ", palette::GOFUN); // unsupported: draws '?'
        g.render();
        let base: Vec<u8> = (0..8).map(|x| g.get_pixel(x, 180)).collect();
        let voiced: Vec<u8> = (8..16).map(|x| g.get_pixel(x, 180)).collect();
        assert!(base.contains(&palette::GOFUN));
        assert_ne!(base, voiced);
        assert_eq!(g.get_pixel(17, 180), palette::GOFUN); // '?' top arc
    }

    #[test]
    fn blit_tile_and_color_key() {
        let mut g = Gpu::new();
        let mut tile = vec![palette::SUMI; TILE_PIXELS];
        tile[0] = palette::BENI;
        g.load_tiles(5, &tile);
        g.begin_frame();
        g.blit_tile(100, 100, 5, false, false);
        g.blit_tile(120, 100, 5, true, true);
        g.render();
        assert_eq!(g.get_pixel(100, 100), palette::BENI);
        assert_eq!(g.get_pixel(101, 100), palette::SUMI);
        assert_eq!(g.get_pixel(127, 107), palette::BENI);
        // Change the key: now SUMI pixels in the tile are opaque, BENI is not.
        g.set_color_key(palette::BENI);
        assert_eq!(g.color_key(), palette::BENI);
        g.clear(palette::GOFUN);
        g.begin_frame();
        g.blit_tile(100, 100, 5, false, false);
        g.render();
        assert_eq!(g.get_pixel(100, 100), palette::GOFUN);
        assert_eq!(g.get_pixel(101, 100), palette::SUMI);
        assert_eq!(g.tile_pixel(5, 0, 0), palette::BENI);
    }

    #[test]
    fn line_scroll_shifts_individual_lines() {
        let mut g = Gpu::new();
        let mut tile = vec![palette::SUMI; TILE_PIXELS];
        for py in 0..TILE_SIZE {
            tile[py * TILE_SIZE] = palette::KI; // left column of the tile
        }
        g.load_tiles(1, &tile);
        g.bg[0].enabled = true;
        g.bg[0].set(0, 0, 1);
        g.bg[0].set_line_scroll(|y| if y == 3 { -5 } else { 0 });
        assert_eq!(g.bg[0].line_scroll_x.len(), SCREEN_H);
        g.render();
        assert_eq!(g.get_pixel(0, 0), palette::KI);
        assert_eq!(g.get_pixel(5, 3), palette::KI);
        assert_eq!(g.get_pixel(0, 3), palette::SUMI);
        g.bg[0].clear_line_scroll();
        g.render();
        assert_eq!(g.get_pixel(0, 3), palette::KI);
        assert_eq!(g.bg[0].line_scroll_x, Vec::<i32>::new());
    }

    #[test]
    fn window_masks_columns() {
        let mut g = Gpu::new();
        g.load_tiles(1, &solid_tile(palette::KI));
        g.bg[0].enabled = true;
        g.bg[0].fill(1);
        g.bg[0].window = Some(Window::new(100, 150, true));
        g.render();
        assert_eq!(g.get_pixel(99, 10), palette::SUMI);
        assert_eq!(g.get_pixel(100, 10), palette::KI);
        assert_eq!(g.get_pixel(149, 10), palette::KI);
        assert_eq!(g.get_pixel(150, 10), palette::SUMI);
        g.bg[0].window = Some(Window::new(100, 150, false));
        g.render();
        assert_eq!(g.get_pixel(99, 10), palette::KI);
        assert_eq!(g.get_pixel(120, 10), palette::SUMI);
    }

    #[test]
    fn mosaic_blocks_layers_but_not_overlay() {
        let mut g = Gpu::new();
        let mut tile = vec![palette::SUMI; TILE_PIXELS];
        tile[0] = palette::KI; // single top-left pixel
        g.load_tiles(1, &tile);
        g.bg[0].enabled = true;
        g.bg[0].fill(1);
        g.set_mosaic(4);
        assert_eq!(g.mosaic(), 4);
        g.begin_frame();
        g.pixel(2, 2, palette::BENI);
        g.render();
        // The 4x4 block at the origin is filled with the top-left pixel...
        assert_eq!(g.get_pixel(3, 3), palette::KI);
        assert_eq!(g.get_pixel(4, 0), palette::SUMI);
        // ...except where the overlay draws.
        assert_eq!(g.get_pixel(2, 2), palette::BENI);
        g.set_mosaic(0);
        assert_eq!(g.mosaic(), 1);
        g.set_mosaic(99);
        assert_eq!(g.mosaic(), 16);
        g.set_mosaic(1);
        g.render();
        assert_eq!(g.get_pixel(3, 3), palette::SUMI);
    }

    #[test]
    fn remap_affects_rgba_only() {
        let mut g = Gpu::new();
        g.clear(palette::AI);
        g.pal(palette::AI, palette::KI);
        g.render();
        assert_eq!(g.get_pixel(0, 0), palette::AI);
        assert_eq!(&g.rgba()[..3], &palette::rgb(palette::KI));
        g.pal_reset();
        g.render();
        assert_eq!(&g.rgba()[..3], &palette::rgb(palette::AI));
    }

    #[test]
    fn screenshot_and_hash() {
        let mut g = Gpu::new();
        g.render();
        let a = g.frame_hash();
        let png = g.screenshot_png();
        assert_eq!(&png[1..4], b"PNG");
        g.clear(palette::KI);
        g.render();
        assert_ne!(a, g.frame_hash());
    }

    #[test]
    fn load_tiles_clamps_at_end_of_memory() {
        let mut g = Gpu::new();
        g.load_tiles(TILE_COUNT - 1, &[7u8; TILE_PIXELS * 2]);
        assert_eq!(g.tile_pixel((TILE_COUNT - 1) as u16, 7, 7), 7);
        g.load_tiles(TILE_COUNT + 5, &[7u8; 4]);
        assert_eq!(g.tile_pixel(5, 0, 0), 0);
    }
}
