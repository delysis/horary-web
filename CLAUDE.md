# Claude instructions for horary-web

## Regression tests

Every time you fix a bug, add a test to `src/__tests__/chartCalc.test.ts` (or a new test file if the bug is in a component) that would have caught it. The test must fail on the broken code and pass on the fix. Run `npm test` after adding it to confirm.

Do not commit a bug fix without a corresponding regression test.
