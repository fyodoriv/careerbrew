#!/usr/bin/env bash
# US-28: import my company's real promotion criteria and grade my evidence against them.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cat > "$SCRATCH/ladder.yaml" <<'YAML'
id: initech-eng
name: Initech engineering ladder (synthetic)
levels: [l4, l5]
competencies:
  - {id: delivery, name: Delivery, weight: 3, effort: 1, signals: {tags: [delivery]}, expect: {l4: 2, l5: 4}, how_to_close: Ship more.}
  - {id: design, name: Design, weight: 3, effort: 2, signals: {tags: [design]}, expect: {l4: 1, l5: 2}, how_to_close: Write RFCs.}
YAML
expect "$(cb ladder import "$SCRATCH/ladder.yaml")" "imported ladder initech-eng" "import"
expect "$(cb ladder list)" "initech-eng" "listed"
cb work note "feat: shipped a thing" --tags delivery >/dev/null
expect "$(cb work gaps --ladder initech-eng --level l5)" "design" "gaps against the company bar"
printf 'id: broken\nname: B\nlevels: [a, b]\ncompetencies:\n  - {id: x, name: X, weight: 1, signals: {}, expect: {a: 1}}\n' > "$SCRATCH/bad.yaml"
expect "$(cb ladder import "$SCRATCH/bad.yaml" 2>&1 || true)" "competency x has no expectation for levels ['b']" "error case: incomplete ladder refused"
echo "ok US-28 ladder import"
