# Contributing to Battlefront Eternal

Bug reports, fixes, tests and documentation are all welcome.

## Ground rules

- **No original game files.** Never commit anything from the games or
  extracted from them: ISOs, `.lvl` archives, textures, models, sounds,
  movies, localized text, scripts or dumps of any of these. Game files stay
  in `data/game/`; prepared and extracted data stays in `extract/` and
  `public/maps`, `public/sides`, `public/common` and `public/audio`. Git
  ignores all of these folders.
- **No copied or decompiled code.** Do not contribute code or text taken from
  the original games' executables, decompiled or disassembled, or from any
  other software whose license does not allow it. Describe behavior in your
  own words and write the code yourself.
- **Licensing.** This repository is licensed under GPL-3.0-only. By opening a
  pull request you agree that your contribution is licensed under the same
  terms, and you confirm you have the right to license it that way.

## Getting set up

[docs/development.md](docs/development.md) covers setup, preparing data
from your own copy of the game, controls and testing. `npm test` runs the
unit tests without any game files. Browser and desktop checks need data
prepared from your own Steam or GOG copy.

## Pull requests

- Keep a pull request to one change, and add a test where you can.
- Run `npm test` before opening it.
- Say what changed and how you checked it, including which maps you tried.
- In player-facing text, say what works today separately from what is
  planned, and don't use em dashes.

## Reporting bugs

Open an issue with:

- the version line from the bottom right of the main menu;
- your platform (Windows version, or Linux distribution);
- whether your copy of the game is from Steam or GOG;
- the steps that reproduce the problem.

Report security problems privately instead: see [SECURITY.md](SECURITY.md).
