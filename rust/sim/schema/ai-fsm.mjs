// Lifecycle callbacks for the shared controller and agent state driver.
export default {
  enums: { AiFsmOp: ['exit', 'enter', 'tick', 'runaway'] },
  records: { AiFsmArgs: { op: 'enum:AiFsmOp' } },
  imports: { aiFsm: { args: 'AiFsmArgs', result: null } },
};
