// Things lying about (src/items.js): the mines and power-ups the battle's
// Items holds, thrown by dispensers, dropped by the dead or placed by the
// world, every field each item gets, with what only a page's drawing reads
// listed apart (`skip`). docs/sim-store.md says how a schema is written;
// tools/sim-schema.mjs makes the code from it.
export default {
  unions: {
    // Whose an item is (Items.spawn `owner`): the unit that threw it from
    // a dispenser, the page's own player, or nobody (a drop at a death, a
    // power-up the world placed).
    ItemOwner: ['null', 'lit:player', 'ref:unit'],
  },

  kinds: {
    // The battle's Items (items.js Items): the items in the order they are
    // stepped (and filtered, keeping order) each tick, and each dispenser's
    // own, oldest first, by the weapon's class and the life that threw them
    // ("class:unit:N:generation", Items.dispense).
    items: {
      singleton: true,
      fields: {
        items: 'list:item',
        owned: 'map:list:item',
      },
      skip: {
        scene: 'the page\'s scene, for the models it draws',
        collision: 'the collision world, reached through the boundary (src/ordnance-world.js)',
        effects: 'effects, drawn by the page',
        lights: 'lights, drawn by the page',
        model: 'models, made for the page',
        explode: 'the battle\'s explosion, reached through the boundary',
        water: 'the water, reached through the boundary',
        pools: 'how many of each base the world holds at most: the battle\'s configuration, given with its definitions',
        views: 'the items a page online shows (network-entity-items.js)',
        world: 'the boundary itself (src/ordnance-world.js)',
        random: 'the battle\'s gameplay stream, drawn through the boundary',
        hit: 'scratch for a cast',
        ray: 'scratch',
        start: 'scratch',
        wet: 'scratch',
        forward: 'scratch',
        right: 'scratch',
        basis: 'scratch',
      },
    },

    // A mine or power-up (items.js itemState(), spawn(), fly()).
    item: {
      home: 'items.items',
      fields: {
        def: 'def:item',
        mine: 'bool',
        position: 'vec3',
        previous: 'vec3',
        velocity: 'vec3',
        yaw: 'f64',
        up: 'vec3',
        landed: 'bool',
        sinking: 'bool',
        // Its model's size and middle (the largest of its parts' bounding
        // spheres), and the model's orientation: a blast's reach and where
        // a power-up is taken from. Without a model: 0.3 m, and no middle
        // or orientation.
        radius: 'f64',
        center: 'vec3|null',
        turn: 'quat|null',
        owner: 'union:ItemOwner',
        team: 'f64',
        age: 'f64',
        life: 'f64',
        health: 'f64',
        gone: 'bool|undefined',
      },
      skip: {
        mesh: 'its model, made to measure it and drawn by the page',
        light: 'colour from the definition, for drawing',
      },
    },

    // Kinds the items refer to, owned elsewhere.
    unit: { key: 'id', class: 'Unit' },
  },
};
