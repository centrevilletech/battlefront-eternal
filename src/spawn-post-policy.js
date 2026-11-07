// Read-only post policy for the page's spawn screen. Native spawning owns
// authoritative choices; these readers use published post and path facts.
import { postAI } from './post-data.js';
const f32=Math.fround;
const FLT_MAX=3.4028234663852886e38;

// Posts the 2004 game's post iterator visits: the active ones, in order.
export function activePosts(posts) {
  return posts.filter((post) => post.active !== false);
}


export function ownerOf(post) {
  return post.team | 0;
}


// Can `team` spawn at this post? Its own post with a spawn path or class
// spawn points, or a post held by a team friendly to it that has an ally
// path. Neutral and inactive posts never qualify.
export function friendlySpawn(post, team, relation) {
  if (post.active === false) return false;
  const owner = ownerOf(post);
  if (!owner) return false;
  const ai = postAI(post);
  if (owner === team) return !!ai.spawnPath?.present || ai.classSpawnPoints > 0;
  return relation(owner, team) > 0 && !!ai.allyPath?.present;
}


function distanceSq(a, b) {
  const x = a.x - b.x;
  const y = a.y - b.y;
  const z = a.z - b.z;
  return f32(x * x + y * y + z * z);
}

// The human's post when the current one no longer serves: the eligible post
// nearest the old one; with no post yet, the eligible post nearest any post
// held by an enemy of the human's team (nothing when no enemy holds a post).
export function humanSpawnPost(post, { posts, team, relation }) {
  const list = activePosts(posts);
  if (post && post.active !== false) {
    if (friendlySpawn(post, team, relation)) return post;
    let best = FLT_MAX;
    let pick = null;
    for (const other of list) {
      if (!friendlySpawn(other, team, relation)) continue;
      const d = distanceSq(other.position, post.position);
      if (best > d) {
        best = d;
        pick = other;
      }
    }
    return pick;
  }
  let best = FLT_MAX;
  let pick = null;
  for (const other of list) {
    if (!friendlySpawn(other, team, relation)) continue;
    for (const enemy of list) {
      if (!(relation(team, ownerOf(enemy)) < 0)) continue;
      const d = distanceSq(other.position, enemy.position);
      if (best > d) {
        best = d;
        pick = other;
      }
    }
  }
  return pick;
}

