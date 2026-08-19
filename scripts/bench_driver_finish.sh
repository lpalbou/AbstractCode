#!/bin/bash
# Drive the benchmark to completion unattended:
#   phase 1: wait for the running 15-cell matrix
#   phase 2: launch lane A (specstd on Zelda x3) + lane B (thin A/B: spec vs
#            specstd x3 each) in parallel, 2 workers each (4 concurrent total)
#   phase 3: behavioral scoring on every new root
#   phase 4: visual review (+VLM once the relay is free) on every new product
#   phase 5: rebuild the web page
# Each phase prints a PHASE line for the monitor; any failure prints FAILED.
set -u
cd /Users/albou/tmp/abstractframework/abstractcode-tui

echo "PHASE 1: waiting for the 15-cell matrix"
while pgrep -f "bench_workflows.py$" >/dev/null 2>&1 || pgrep -f "bench_workflows.py >" >/dev/null 2>&1; do sleep 20; done
# settle: let the last runs.json write land
sleep 5
V=$(grep -cE '^    (VALID|DISCARD)' untracked/workflow-bench.log 2>/dev/null || echo 0)
echo "PHASE 1 DONE: matrix verdicts=$V/15"

echo "PHASE 2: launching lane A (specstd zelda x3) + lane B (thin A/B x6)"
WB_OUT=workflow-bench-specstd WB_PARALLEL=2 \
  nohup python3 scripts/bench_workflows.py --arms specstd --repeats 3 \
  > untracked/lane-specstd.log 2>&1 &
A=$!
WB_OUT=workflow-bench-thin WB_PROMPT=thin WB_PARALLEL=2 \
  nohup python3 scripts/bench_workflows.py --arms spec,specstd --repeats 3 \
  > untracked/lane-thin.log 2>&1 &
B=$!
wait $A; RA=$?
wait $B; RB=$?
echo "PHASE 2 DONE: laneA=$RA laneB=$RB"
grep -E '^    (VALID|DISCARD|INFRA)' untracked/lane-specstd.log | sed 's/^/  laneA /'
grep -E '^    (VALID|DISCARD|INFRA)' untracked/lane-thin.log | sed 's/^/  laneB /'

echo "PHASE 3: behavioral scoring"
for root in workflow-bench workflow-bench-specstd workflow-bench-thin; do
  [ -f "untracked/$root/runs.json" ] || continue
  python3 scripts/zelda_review_score.py --root "untracked/$root" \
    > "untracked/$root/score-run.log" 2>&1 \
    && echo "  scored $root" || echo "  FAILED scoring $root"
done

echo "PHASE 4: visual review + VLM"
python3 scripts/zelda_visual_review.py --vlm \
  untracked/workflow-bench/*-product \
  untracked/workflow-bench-specstd/*-product \
  untracked/workflow-bench-thin/*-product \
  > untracked/vr-final.log 2>&1 && echo "  visual review done" || echo "  FAILED visual review"

echo "PHASE 5: rebuild page"
pkill -f bench_matrix_page 2>/dev/null; sleep 1
python3 scripts/bench_matrix_page.py --no-serve >/dev/null 2>&1
nohup python3 scripts/bench_matrix_page.py --port 8899 > untracked/bench-site.log 2>&1 &
sleep 3
CODE=$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8899/ 2>/dev/null)
echo "PHASE 5 DONE: page=$CODE"
echo "ALL PHASES COMPLETE"
