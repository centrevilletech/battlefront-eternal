// The map's own ambience, as the 2004 game sets it up:
//
// - Static emitters: placed objects with a Sound property (Theed's 36
//   com_snd_amb_static: 31 "canal" and 5 "fountain"), each a looping sound
//   at its place. MinDistance and MaxDistance (which sets MuteDistance too)
//   replace the sound's; the class's own are 1 and 10. Out of reach they keep
//   time silently (virtual voices).
// - Stream regions, named "soundstream <properties> <v>": an emitter of that
//   stream at the region's centre, its MaxDistance and MuteDistance the
//   region's size (|SIZE|, whatever its shape) and its MinDistance that over
//   v. Theed's "soundstream nab_amb_city 1.0" is 400.5 m: the city's quad
//   bed at full level anywhere within 400.5 m of the centre, silent beyond.
//   A multichannel stream isn't panned, only set by distance.

export class Ambience {
  constructor(audio, world) {
    this.audio = audio;
    this.emitters = [];
    for (const e of world.emitters || []) {
      const props = e.props || {};
      const overrides = {
        MinDistance: Number(props.MinDistance ?? 1),
        MaxDistance: Number(props.MaxDistance ?? 10),
      };
      overrides.MuteDistance = overrides.MaxDistance;
      const position = { x: e.position[0], y: e.position[1], z: e.position[2] };
      const handle = audio.engine.play(props.Sound || props.SoundStream, { position, overrides, loop: true, category: "ambience" });
      if (handle) this.emitters.push(handle);
    }
    this.regions = (world.regions || [])
      .filter((r) => r.name.split(/\s+/)[0].toLowerCase() === "soundstream")
      .map((r) => {
        const [, name, v] = r.name.split(/\s+/);
        const R = Math.hypot(...r.size);
        const scale = Number(v);
        return {
          name,
          position: { x: r.matrix[12], y: r.matrix[13], z: r.matrix[14] },
          overrides: { MinDistance: scale > 0 ? R / scale : R, MaxDistance: R, MuteDistance: R },
          voice: null,
        };
      });
  }

  update() {
    // Each stream region's emitter runs throughout; its distance sets it.
    for (const region of this.regions) {
      if (region.voice && !region.voice.done) continue;
      region.voice = this.audio.streams.play(region.name, {
        category: "ambience",
        position: region.position,
        overrides: region.overrides,
        loop: true,
        fadeIn: 0.5,
      });
    }
  }

  stop() {
    for (const e of this.emitters) e.stop(0.2);
    for (const r of this.regions) r.voice?.stop(0.2);
  }
}
