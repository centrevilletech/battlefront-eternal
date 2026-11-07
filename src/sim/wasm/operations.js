// Low-level fixtures and normal worlds share the same concrete operations.
import { createWireRegistryBridge } from './runtime/wire-registry.js';
import { createWirePresentationBridge } from './runtime/wire-presentation.js';
import { createWireCosmeticsBridge } from './runtime/wire-cosmetics.js';
import { createWireBudgetBridge,createWireHostBridge,createWireStaggerBridge } from './runtime/wire-host.js';
import { createWireShotCounters } from './runtime/wire-shot-counters.js';
import { createWireSemanticBridge } from './runtime/wire-semantic.js';
import { createWireBridge } from './runtime/wire-sim.js';
import { WireEntityEdges } from './runtime/wire-edges.js';
import { WireCombat } from './runtime/wire-combat.js';
import { InputMailbox, bindSpawnSelection } from './runtime/input-mailbox.js';
import {createWorldSim} from './runtime/world-sim.js';
import {createStoreSource} from './runtime/wire-store-source.js';
export const operations=Object.freeze({
  world: backend=>createWorldSim(backend.instance),
  bindSpawnSelection: (backend,...args)=>bindSpawnSelection(...args),
  createWireRegistryBridge: (backend,...args)=>createWireRegistryBridge(...args),
  createWirePresentationBridge: (backend,...args)=>createWirePresentationBridge(...args),
  createWireCosmeticsBridge: (backend,...args)=>createWireCosmeticsBridge(...args),
  createWireBudgetBridge: (backend,...args)=>createWireBudgetBridge(...args),
  createWireHostBridge: (backend,...args)=>createWireHostBridge(...args),
  createWireStaggerBridge: (backend,...args)=>createWireStaggerBridge(...args),
  createWireShotCounters: (backend,...args)=>createWireShotCounters(...args),
  createWireSemanticBridge: (backend,...args)=>createWireSemanticBridge(...args),
  createWireBridge: (backend,...args)=>createWireBridge(...args),
  createStoreSource: (backend,...args)=>createStoreSource(...args),
  WireEntityEdges: (backend,...args)=>new WireEntityEdges(...args),
  WireCombat: (backend,...args)=>new WireCombat(...args),
  InputMailbox: (backend,...args)=>new InputMailbox(...args),
});
