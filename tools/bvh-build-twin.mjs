// Synthetic packed-tree oracle for the native test. No game files needed.
import { builderCases, recordFor } from '../tests/fixtures/bvh-build-cases.js';
for (const fixture of builderCases()) process.stdout.write(JSON.stringify(recordFor(fixture)) + '\n');
