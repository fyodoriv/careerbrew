const assert = require("node:assert/strict");
const fs = require("node:fs");

const report = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const specs = report.suites.flatMap((suite) => suite.specs);
assert.equal(specs.length, 3);
assert.equal(report.stats.unexpected, 0);
assert.equal(report.stats.flaky, 0);
assert.equal(report.stats.skipped, 0);
for (const [title, expected, sentinel] of [
  ["route callback error is reported", "failed", true],
  ["a clean visit completes", "passed", false],
  ["standup route callback error is reported during drain", "failed", true],
]) {
  const spec = specs.find((item) => item.title === title);
  assert.ok(spec);
  assert.equal(spec.tests.length, 1);
  const test = spec.tests[0];
  assert.equal(test.expectedStatus, expected);
  assert.equal(test.results.length, 1);
  assert.equal(test.results[0].status, expected);
  assert.equal(JSON.stringify(test.results[0].errors).includes(title.startsWith("standup") ? "synthetic-standup-route-error-sentinel" : "synthetic-route-error-sentinel"), sentinel);
}
