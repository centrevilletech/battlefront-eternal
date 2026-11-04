// Queries made by an owned prediction session. The host resolves foreign
// collision and world objects while the session keeps its private state.
export default {
  records: {
    PredictionHullQuery: { session: 'f64', op: 'u32', args: 'list:f64' },
    PredictionHullReply: { values: 'list:f64' },
  },
  imports: {
    predictionHullQuery: { args: 'PredictionHullQuery', result: 'PredictionHullReply' },
  },
};
