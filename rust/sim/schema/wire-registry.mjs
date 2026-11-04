// Content names and input profiles cross once when a host is created.
export default {
  records: {
    WrName: { value: 'str|null' },
    WrBinding: { action: 'str', valid: 'bool', codes: 'list:rec:WrName' },
    WrProfile: { id: 'f64', idValid: 'bool', valid: 'bool', bindings: 'list:rec:WrBinding' },
    WrInputs: { epoch: 'f64', epochValid: 'bool', codesValid: 'bool', profilesValid: 'bool', codes: 'list:rec:WrName', profiles: 'list:rec:WrProfile' },
    WrTables: { classes: 'list:rec:WrName', weapons: 'list:rec:WrName', posts: 'list:rec:WrName' },
    WrResult: { error: 'str|null', codes: 'list:rec:WrName', profiles: 'list:rec:WrProfile', classes: 'list:rec:WrName', weapons: 'list:rec:WrName', posts: 'list:rec:WrName' },
  },
};
