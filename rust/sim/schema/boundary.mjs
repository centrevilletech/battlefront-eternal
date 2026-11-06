// Shared block headers and numeric limits have one generated definition.
// They describe memory, not gameplay state or an optional simulation mode.
export default {
  constants:{NO_RANDOM_STREAM:4294967295,PHASE_INPUT_WORDS:262144,PHASE_DELTA_WORDS:4096},
  records:{BlockHeader:{$layout:true,rows:'u32',stride:'u32',heap:'u32',heapWords:'u32',retired:'u32'}},
};
