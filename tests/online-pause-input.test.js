// Paused online, the battle plays on and his commands go out still
// (stillCommand, src/runtime/commands.js). These follow one through the
// client's sender, the wire and the server's mailbox to the flight keys a
// hull takes, since the server keeps whatever pad it last had.
import test from 'node:test';
import assert from 'node:assert/strict';
import { stillCommand } from '../src/runtime/commands.js';
import { EntityInputSender } from '../src/runtime/network-entity-sender.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES, decodeEntityInput } from '../src/runtime/network-entity-input.js';
import { InputMailbox } from '../src/runtime/transport.js';
import { flightInput, flightBindings } from '../src/flight/controls.js';

const registry = { inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES, classIds: new Map(), postIds: new Map() };
// A pad as src/game.js reads it: four axes, seventeen buttons.
const pad = (axes, pressed = []) => ({ axes, buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: pressed.includes(i) })) });
// A pilot's command as Controls.command and decorateCommand make it.
const flying = (sequence, extra = {}) => ({ epoch: 1, generation: 1, sequence, tick: sequence, down: [], actions: [], look: [],
  flightSettings: { sensitivity: 1, curve: 1, invert: false, bindings: {} }, ...extra });

// The client's sender to the server's mailbox: what the hull takes at `tick`.
function wire() {
  const mailbox = new InputMailbox(), packets = [];
  const sender = new EntityInputSender({ registry, send: (bytes) => {
    const packet = decodeEntityInput(bytes, registry);
    packets.push(packet);
    for (const command of packet.commands) mailbox.push({ ...command, epoch: packet.epoch, generation: packet.generation });
    return true;
  } });
  const keys = (tick) => {
    const command = mailbox.take(tick, 1);
    return { command, input: flightInput({ down: new Set(command.down), take: () => false }, flightBindings(command.flightSettings?.bindings), command.gamepad) };
  };
  return { sender, packets, keys };
}

test('a stick held into the pause menu online is centred on the server, not left deflected', () => {
  const { sender, packets, keys } = wire();
  // Flying with the stick over, throttle on the stick, boost (button 11) and the trigger held.
  sender.send([flying(1, { down: ['KeyW', 'Mouse0'], gamepad: pad([0.9, -0.8, 0.6, -1], [11]) })]);
  const before = keys(1).input;
  assert.equal(before.stickX, 0.9);
  assert.equal(before.boost, true);

  // The pause menu opens: the keys are let go, the pad is still held over.
  sender.send([stillCommand(flying(2, { down: ['KeyW', 'Mouse0'], look: [[40, -12]], actions: ['use'],
    gamepad: pad([0.9, -0.8, 0.6, -1], [11]), human: [{ kind: 'suicide' }] }))]);
  const { command, input } = keys(2);
  assert.deepEqual(command.down, []);
  assert.deepEqual(command.gamepad.axes, [0, 0, 0, 0], 'the server holds a centred pad');
  assert.ok(command.gamepad.buttons.every((button) => !button.pressed), 'and no button down');
  assert.ok(input.forward === 0 && input.right === 0, 'no throttle or roll'); // -0 from the pad's term

  assert.equal('stickX' in input || 'stickY' in input, false, "the hull's own stick returns to centre");
  assert.equal(input.boost, false);
  assert.equal(input.trick, false);
  // The look total doesn't move, and of the edges only RESPAWN and the trigger's release go.
  assert.deepEqual(packets.at(-1).commands.at(-1).lookTotal, packets[0].commands.at(-1).lookTotal);
  assert.deepEqual(packets.at(-1).edges.filter((edge) => edge.id > packets[0].edges.length).map((edge) => [edge.kind, edge.pressed ?? null]),
    [['suicide', null], ['fire', false]]);
});

test('a command without a pad leaves the server holding the last one (why stillCommand centres it)', () => {
  const { sender, keys } = wire();
  sender.send([flying(1, { gamepad: pad([0.9, -0.8, 0.6, -1], [11]) })]);
  sender.send([flying(2, { gamepad: null })]);
  const { command, input } = keys(2);
  assert.deepEqual(command.gamepad.axes, [0.9, -0.8, 0.6, -1]);
  assert.equal(input.stickX, 0.9);
  assert.equal(input.boost, true);
});
