// Serverless browser netplay for KAGAMI.
//
// Two browsers connect directly over a WebRTC data channel configured to
// behave like UDP (unordered, no retransmits), which is exactly what the
// lockstep and rollback sessions inside the cartridge expect. Signaling is
// an offer code and an answer code that the players paste to each other
// over any chat. No server is involved; a public STUN server is listed
// only so peers behind NAT can learn their own addresses.
//
//   const link = await hostLink();            // -> link.code to send to the guest
//   await link.accept(answerCode);            // guest's reply
//
//   const link = await joinLink(offerCode);   // -> link.code to send back
//
// Then `attachNetplay(console, link, { localPlayer, players, delay, rollback })`.

const ICE = [{ urls: 'stun:stun.l.google.com:19302' }];

function encode(desc) {
  return btoa(JSON.stringify({ type: desc.type, sdp: desc.sdp }));
}
function decode(code) {
  return JSON.parse(atob(code.trim()));
}

async function gathered(pc) {
  if (pc.iceGatheringState === 'complete') return;
  await new Promise((resolve) => {
    const check = () => { if (pc.iceGatheringState === 'complete') { pc.removeEventListener('icegatheringstatechange', check); resolve(); } };
    pc.addEventListener('icegatheringstatechange', check);
    setTimeout(resolve, 3000); // candidates so far are enough on a LAN
  });
}

function makeLink(pc, channelPromise) {
  const link = {
    pc,
    channel: null,
    code: '',
    opened: channelPromise.then((ch) => { link.channel = ch; return ch; }),
    onmessage: null,
    send(bytes) { if (link.channel && link.channel.readyState === 'open') link.channel.send(bytes); },
    close() { try { pc.close(); } catch { /* already closed */ } },
    get state() { return link.channel ? link.channel.readyState : pc.connectionState; },
  };
  channelPromise.then((ch) => {
    ch.binaryType = 'arraybuffer';
    ch.onmessage = (e) => { if (link.onmessage) link.onmessage(new Uint8Array(e.data)); };
  });
  return link;
}

/** The host creates the connection and an offer code. */
export async function hostLink({ ice = ICE } = {}) {
  const pc = new RTCPeerConnection({ iceServers: ice });
  const ch = pc.createDataChannel('kagami', { ordered: false, maxRetransmits: 0 });
  const opened = new Promise((resolve) => { ch.onopen = () => resolve(ch); });
  const link = makeLink(pc, opened);
  await pc.setLocalDescription(await pc.createOffer());
  await gathered(pc);
  link.code = encode(pc.localDescription);
  link.accept = async (answerCode) => { await pc.setRemoteDescription(decode(answerCode)); await link.opened; };
  return link;
}

/** The guest takes the offer code and produces an answer code. */
export async function joinLink(offerCode, { ice = ICE } = {}) {
  const pc = new RTCPeerConnection({ iceServers: ice });
  const opened = new Promise((resolve) => {
    pc.ondatachannel = (e) => { const ch = e.channel; if (ch.readyState === 'open') resolve(ch); else ch.onopen = () => resolve(ch); };
  });
  const link = makeLink(pc, opened);
  await pc.setRemoteDescription(decode(offerCode));
  await pc.setLocalDescription(await pc.createAnswer());
  await gathered(pc);
  link.code = encode(pc.localDescription);
  return link;
}

/**
 * Starts a netplay session in the cartridge and pumps datagrams between it
 * and the link. Returns a handle with `stop()`.
 */
export function attachNetplay(console_, link, { localPlayer = 0, players = 2, delay = 2, rollback = false, prediction = 8 } = {}) {
  console_.startNet({ localPlayer, players, delay, prediction: rollback ? prediction : 0, send: (to, bytes) => link.send(bytes) });
  link.onmessage = (bytes) => console_.receive(bytes);
  return {
    stop() { link.onmessage = null; console_.stopNet(); link.close(); },
  };
}
