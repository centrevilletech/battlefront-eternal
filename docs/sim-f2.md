# Vehicle phase ownership

The F2 opt-in is `?f2=rust` in a headless world, or `BFE_F2=rust` under Node.
It is independent of the older hull and seat experiments and defaults off.

The final F2 boundary covers pads, hulls, flight, rigs, leases, mounted weapons,
tethers and rider writeback. The first milestone transfers hull and flight
scheduling; seats, firing, tethers and riders are foreign operations until their
remaining steps land.

Rust owns each vehicle's position, previous position, velocity, heading and rates,
support, health, dying clock, water clock, sleeping flag, flight checkpoint,
wheels, tick capture and retained rig. The generated vehicle slab stores hull and
flight state. Retained rigs are keyed by the slab handle and gameplay life.
A respawn keeps the Vehicle and target objects, increments the target generation,
and resets the new life. Retirement invalidates the slab handle before reuse.

JavaScript readers still use Vehicle methods and their existing vectors. The
published fields are the vehicle schema's mirror fields plus compact pose data.
The full rig remains published during this milestone because the unported firing,
boarding and rider code reads its nodes. That output can shrink after those users
move. Full hull imports are construction-only, never a tick input.

Foreign inputs are registry facts and team relations, body flags, frozen crew
input, seat look and lease facts, and firing recoil inputs. JS still owns weapons,
lease lifecycle and tethers. Those are explicit boundary inputs, not permission to
write physics or flight mirrors. AI intents remain JS-owned. Damage, health,
supply and wake writes become synchronous scalar commands; commands publish the
touched mirror before returning. A stale handle or gameplay life is rejected.

The scheduler order is clamp/time, spawn clocks, latch/exits, ground capture,
flight, ground substeps, dying, all aim passes, all gameplay render passes, then
foreign fire, tether and rider work. Render's leg easing and the ground leftover
alpha are gameplay state and must remain. No foreign callback may retain a Rust
borrow or reimport a whole fleet.

Boundary counters distinguish phase calls, lifecycle calls, commands and debug
calls. Field counts are scalar words actually copied, including list payloads and
rig output, rather than a count of property names. Debug mirror checks compare
published values with `Object.is`, so signed zero and NaN retain their meaning.
They do not install property traps or alter JavaScript intrinsics.

Later seats work belongs in `f2/seats`; firing and tow work belongs in `f2/firing`.
The scheduler owns ordering, lifecycle owns birth/retirement, and the integrator
owns the schema, shared publication and game wiring. Changes to those shared
interfaces must land before parallel seats and firing work begins.
