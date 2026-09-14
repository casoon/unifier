// @ts-check
import casoonPages from '@casoon/pages-theme';
import { defineConfig } from 'astro/config';

// Project page: https://casoon.github.io/unifier/ — `base` is the GitHub Pages path.
export default defineConfig({
  site: 'https://casoon.github.io/unifier',
  base: '/unifier/',
  integrations: [
    casoonPages({
      name: 'unifier',
      description:
        'Constraint satisfaction and optimization (CSP/COP) modelling and solver framework for Rust.',
      repo: 'casoon/unifier',
      version: '0.3.2',
      license: 'MIT',
      branch: 'master',
      packages: [
        { label: 'crates.io', href: 'https://crates.io/crates/unifier' },
        { label: 'docs.rs', href: 'https://docs.rs/unifier/0.3.2/unifier/' },
      ],
      docsGroups: {
        'getting-started': 'Getting started',
        guides: 'Guides',
        reference: 'Reference',
      },
      changelog: '../CHANGELOG.md',
    }),
  ],
});
