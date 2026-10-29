// The VehicleAI each Classic AI binds (index.js), for the vehicle agents:
// the AI's own ai.vehicles adapter carries only the service names it lists.

const services = new WeakMap();

export function serviceOf(ai) {
  return services.get(ai) ?? null;
}

export function registerService(ai, service) {
  services.set(ai, service);
  return service;
}
