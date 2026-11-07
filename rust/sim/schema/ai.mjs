// Planning state belongs to the native battle owner. These identities let
// Character and body mirrors retain generation-qualified controller handles;
// no scheduler, command queue or movement state crosses this facade.
export default {
  kinds: {
    controller: {},
    seatUnit: { class: 'SeatUnit' },
  },
};
