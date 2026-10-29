// One required wire contract, selected by the mounted engine. This is not a
// capability list. The server must validate its adapter ABI before advertising.
// Protocol 3 is the native UDP transport, BFE-UDP/1 (src/net and
// docs/native-udp-transport.md); protocol 2 was WebRTC data channels.
export const NETWORK_CONTRACT = Object.freeze({
  manifestVersion: 1,
  protocol: 3,
  profile: 'snapshot-delta-v1',
  simulation: 'bfe-sim-27',
  serverAdapterABI: 4,
  ownCorrectionSchema: 11,
});
