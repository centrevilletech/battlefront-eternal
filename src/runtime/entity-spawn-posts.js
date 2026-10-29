import { transportPosts } from '../flight/spawn.js';
// Mobile spawn names are reserved by map slot, independent of which vehicle
// variant currently occupies it: a command vehicle's post (src/mobile-posts.js)
// and a landed transport's take their pad's name. Eligibility and position
// still come from the host.
export const entityPostNames = game => [...game.posts.posts, ...(game.vehicles?.slots || []).map((_, i) => `transport-${i}`)];
export const entitySpawnPosts = game => [...(game.posts.all?.() ?? game.posts.posts).filter(post => post.active !== false), ...transportPosts(game.vehicles)];
