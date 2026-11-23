# Client test data

These Course Packages are owned by the Client test suites. Core and Client App
tests may read them because they exercise the same Client component.
`narration-seek.ogg` is a Client App audio fixture for narration seeking; it is
not a Course Package.

Shared Course Package fixtures used by `cynapse-course` live in
`course/test-data/`; Client tests may read that collection because the Client
depends on the course crate. Client-only fixtures stay here.

They are test inputs, not distributable product content. Repository-level
`courses/` may be removed without changing these tests.
Protocol-wide golden messages belong under `protocol/test-data/` instead.

## Fixture shapes

Counts are Unit nodes with `steps` and referenced Step files. A challenge count
is the number of Steps with a `challenge` object. Keep this table in sync when a
fixture's manifest changes.

| Fixture | Units | Steps | Challenges | Notes |
| --- | ---: | ---: | ---: | --- |
| `challenge-fix-test` | 1 | 1 | 1 | Single Unit; the challenge is also the final course Step |
| `continuous-learning-demo` | 4 | 7 | 0 | Continuous progression across Units |
| `independent-units` | 5 | 5 | 0 | Shared Steps across independent Units |
| `manual-boundary-demo` | 1 | 3 | 0 | No `workspace`; manual Step boundaries |
| `multi-node-demo` | 8 | 8 | 0 | Multiple nodes and shared Step files |
| `multi-unit-demo` | 2 | 2 | 0 | Two Units at the same level |
