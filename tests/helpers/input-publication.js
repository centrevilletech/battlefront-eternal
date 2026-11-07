// The live sender lends its publication until a synchronous transport returns.
// Simulated delayed links must own that publication, just as they own bytes.
export function capturePublication(value) {
  if (value.type === 'inputs') return {type:'inputs',commands:value.commands.map(command=>structuredClone(command.toJSON()))};
  return structuredClone(value.toJSON());
}
