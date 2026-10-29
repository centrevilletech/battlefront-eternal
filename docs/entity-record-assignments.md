# Entity record assignments

This is the shared allocation table for the single required entity protocol.
A number identifies one record body; it is not a negotiated capability. Change
`NETWORK_CONTRACT.simulation` when a client cannot decode newly emitted records.
Keep existing meanings stable across development branches and update this table
before allocating another kind.

The twelve kind numbers below are stable. Body schemas added to them
(simulation 17 and later):

| Kind | Schema | Meaning | Identity |
| --- | --- | --- | --- |
| Summary (6) | 6 | Unit weapon actions, repair lock and body presentation state | 100000 + unit ID |
| Summary (6) | 7 | Non-turret building and station state | 200000 + building index |
| Summary (6) | 8 | Vehicle tow cable state | 300000 + vehicle ID |
| Summary (6) | 9 | Animated prop that seizes: clip, start, whom it holds (simulation 21) | 400000 + animated prop index |
| Summary (6) | 10 | Trap phase and the tick it began (simulation 24) | 500000 + trap index |
| Combat (8) | 8 | Named presentation event, bounded JSON payload | Combat event ID |

These schema namespaces are independent. Unit and tow records carry the
actor's generation; building records carry the world epoch. Presentation
events expire after 120 ticks and validate actor generations before applying.
Summary schemas 1-5 remain battle, posts, spawn eligibility, roster and
personal records. The current own-player outer schema is 11.

| Kind | Meaning |
| --- | --- |
| 1 | Soldier |
| 2 | Vehicle |
| 3 | Actor tombstone |
| 4 | Seat lease |
| 5 | Own-player correction |
| 6 | World summary |
| 7 | Mount/slot relationship |
| 8 | Combat event or dynamic projectile |
| 9 | Persistent item slot |
| 10 | Recon droid |
| 11 | Stationary turret |
| 12 | Droideka actor |

Record kind numbers and body-schema numbers are different namespaces. Combat
uses its own schema discriminator inside kind 8. The own-player correction
(kind 5) contains a control tag in its outer schema: 0 ordinary body, 1 recon,
2 vehicle passenger, 3 stationary turret and 4 pilot weapon state alongside the
exact hull. It also carries the authoritative team byte. Droideka exact body
schema 10 is a separate inner-body namespace. The mounted manifest must match
the actual outer correction codec export before readiness.

These record bodies are engine-owned under server adapter ABI 4. The server's
fixed packet-envelope assumptions still apply; changing those requires a new
adapter ABI and a coordinated compatible server, as described in
[the contract document](network-contract.md).
