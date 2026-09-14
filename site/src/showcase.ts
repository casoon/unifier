import { ansiToHtml } from '@casoon/pages-theme/ansi';
import type { ShowcaseExample } from '@casoon/pages-theme/showcase';

// The example programs in examples/ are part of the crate (`cargo run --example <name>`).
// Their output is captured into examples/output/ by scripts/regenerate-showcase.sh and
// committed, so every result on this site was produced by unifier itself.
const sources = import.meta.glob<string>('../../examples/*.rs', {
  query: '?raw',
  import: 'default',
  eager: true,
});
const outputs = import.meta.glob<string>('../../examples/output/*.txt', {
  query: '?raw',
  import: 'default',
  eager: true,
});

export function output(example: string): string {
  const found = outputs[`../../examples/output/${example}.txt`];
  if (found === undefined) throw new Error(`Missing captured output: ${example}`);
  return found;
}

function source(example: string): string {
  const found = sources[`../../examples/${example}.rs`];
  if (found === undefined) throw new Error(`Missing example: ${example}`);
  return found;
}

const catalogue = [
  {
    slug: 'eight-queens',
    example: 'nqueens',
    title: 'Eight queens',
    tags: ['CSP', 'Backtracking', 'AllDifferent'],
    description:
      'Place eight queens so that none attacks another. Columns are kept apart by AllDifferent, diagonals by NotEqual with an offset; the backtracking solver returns one valid placement.',
  },
  {
    slug: 'map-colouring',
    example: 'map_coloring',
    title: 'Map colouring',
    tags: ['CSP', 'Backtracking', 'Infeasible'],
    description:
      'The seven Australian states and territories, neighbours in different colours. Three colours are enough; for two the solver exhausts the search space and reports Infeasible.',
  },
  {
    slug: 'job-shop',
    example: 'job_shop',
    title: 'Job-shop makespan',
    tags: ['COP', 'Branch & Bound', 'NoOverlap', 'Precedence'],
    description:
      'Three jobs on three machines, each job a fixed sequence of operations. Branch & Bound minimizes the makespan and proves 11 optimal.',
  },
  {
    slug: 'school-timetable',
    example: 'scheduling_demo',
    title: 'School timetable',
    tags: ['COP', 'Branch & Bound', 'Cumulative', 'Optional activity'],
    description:
      'Activities and resources compiled into Cumulative and NoOverlap constraints, with a calendar exclusion, an optional activity, an alternative lab and a tardiness objective. When several optimal schedules exist, which one is printed can differ between runs.',
  },
];

export const examples: ShowcaseExample[] = catalogue.map(({ example, ...meta }) => ({
  ...meta,
  file: `examples/${example}.rs`,
  input: { code: source(example), lang: 'rust' },
  output: { html: ansiToHtml(output(example)), kind: 'terminal' },
}));
