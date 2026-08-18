#!/usr/bin/env bats
# Issue #544 — ownership fence between `neat-core` (per-sample primitives) and
# NEAT-AI-Backpropagation (the `trainDir` epoch loop: accumulate → apply → MSE
# accept/rollback → journal). Nothing stopped a future "simplify by putting
# trainDir in core" change from pulling journal, CLI apply policy and
# `traceStore` layout into every core consumer, so the fence is asserted here:
# the rule has a documented home, and the sources are swept for the host
# orchestration items it excludes.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS="${REPO_ROOT}/AGENTS.md"
  README="${REPO_ROOT}/README.md"
  CORE_SRC="${REPO_ROOT}/neat-core/src"

  # ONE definition of the host-orchestration item pattern (AGENTS.md oracle
  # rule 4: a gate self-test compiles the live pattern, never a private copy).
  # The sweep over the real sources and the good/bad literal checks below all
  # match against this string, so gutting it fails the literal tests.
  #
  # It matches a Rust *item definition* whose name carries an epoch-loop
  # concept — `fn train_dir_epoch`, `struct EpochJournal`, `mod trace_store` —
  # and deliberately not prose, so a doc comment mentioning "epoch" is free.
  HOST_ITEM_RE='^[[:space:]]*(pub[[:space:]]*(\([^)]*\)[[:space:]]*)?)?(fn|struct|enum|trait|mod|type|const|static)[[:space:]]+[a-z0-9_]*(train_?dir|epoch|journal|trace_?store)'

  # ONE definition of the module basenames that would mean the epoch loop was
  # relocated here. `training_data.rs` / `training_state.rs` /
  # `training_bin_stream.rs` are core primitives and stay.
  HOST_MODULE_RE='^(train|trainer|train_dir|epoch|journal|trace_store)\.rs$'
}

# Body of the H2 named $1, up to the next H2. Fails loud on an empty body so a
# renamed heading cannot make every assertion below pass vacuously.
section_body() {
  local heading="$1" body
  body="$(awk -v h="$heading" '
    index($0, "## ") == 1 { f = (substr($0, 4) == h) ? 1 : 0; next }
    f' "$AGENTS")"
  if [[ -z "${body//[[:space:]]/}" ]]; then
    echo "AGENTS.md has no body under '## ${heading}'" >&2
    return 1
  fi
  printf '%s\n' "$body"
}

# assert_states <text> <ERE> <what the rule must say>
assert_states() {
  local text="$1" pattern="$2" what="$3"
  if [[ ! "$text" =~ $pattern ]]; then
    echo "AGENTS.md does not state ${what} (no /${pattern}/)" >&2
    return 1
  fi
}

# --- The fence itself: neat-core carries no epoch orchestration -------------

@test "neat-core sources define no trainDir/epoch/journal/traceStore item" {
  run grep -rEni "$HOST_ITEM_RE" "$CORE_SRC"
  if [ "$status" -eq 0 ]; then
    echo "host-orchestration items found in neat-core/src:" >&2
    echo "$output" >&2
    echo "Those belong in NEAT-AI-Backpropagation — see AGENTS.md 'Ownership fence'." >&2
  fi
  [ "$status" -ne 0 ]
}

@test "neat-core has no train.rs-style epoch orchestration module" {
  local offenders=""
  local file
  for file in "$CORE_SRC"/*.rs "$CORE_SRC"/*/*.rs; do
    [ -e "$file" ] || continue
    if [[ "$(basename "$file")" =~ $HOST_MODULE_RE ]]; then
      offenders+="${file}"$'\n'
    fi
  done
  [ -z "$offenders" ] || {
    echo "epoch-orchestration modules found: $offenders" >&2
    false
  }
}

# --- The pattern can fail: known-bad and known-good literals ----------------

@test "the item pattern rejects epoch-orchestration definitions" {
  local bad
  for bad in \
    'pub fn train_dir(dir: &Path) -> Result<(), TrainError> {' \
    'pub fn train_epoch_loop(&mut self) -> EpochResult {' \
    '    pub(crate) fn apply_epoch(&mut self) {' \
    'struct EpochJournal {' \
    'pub struct TrainDirOptions {' \
    'mod trace_store;' \
    'const JOURNAL_PATH: &str = ".journal";'
  do
    printf '%s\n' "$bad" | grep -Eqi "$HOST_ITEM_RE" || {
      echo "pattern failed to reject: $bad" >&2
      false
    }
  done
}

@test "the item pattern accepts the primitives core keeps" {
  local good
  for good in \
    "pub fn propagate_topological_loop(input: &PropagateInput<'_>) -> PropagateOutput {" \
    'pub fn mse_mean_streaming(' \
    'pub fn compute_reverse_topological_order(' \
    'pub struct TrainingDataConfig {' \
    'pub mod training_bin_stream;' \
    '/// Initialise persistent training state for an epoch.' \
    '// The trainDir journal stays in NEAT-AI-Backpropagation.'
  do
    printf '%s\n' "$good" | grep -Eqi "$HOST_ITEM_RE" && {
      echo "pattern wrongly rejected: $good" >&2
      false
    }
  done
  true
}

@test "the module pattern rejects train.rs but keeps the training_* primitives" {
  local bad good
  for bad in train.rs trainer.rs train_dir.rs epoch.rs journal.rs trace_store.rs; do
    [[ "$bad" =~ $HOST_MODULE_RE ]] || {
      echo "module pattern failed to reject: $bad" >&2
      false
    }
  done
  for good in training_data.rs training_state.rs training_bin_stream.rs topological_backprop.rs; do
    [[ "$good" =~ $HOST_MODULE_RE ]] && {
      echo "module pattern wrongly rejected: $good" >&2
      false
    }
  done
  true
}

# --- The fence is documented ------------------------------------------------

@test "AGENTS.md carries the ownership fence section" {
  run grep -c '^## Ownership fence (Issue #544)$' "$AGENTS"
  [ "$status" -eq 0 ]
  [ "$output" -eq 1 ]
}

@test "the fence names the per-sample primitives core keeps" {
  local body
  body="$(section_body 'Ownership fence (Issue #544)')"
  assert_states "$body" 'propagate_topological_loop' 'that the per-sample propagate loop stays'
  assert_states "$body" 'propagate_codec' 'that the packed ABI codec stays'
  assert_states "$body" 'mse_mean_streaming' 'that the streaming MSE helper stays'
  assert_states "$body" 'training_data' 'that the training-data iterators stay'
  assert_states "$body" 'topology_ops' 'that the topology helpers stay'
}

@test "the fence names the host orchestration Backpropagation owns" {
  local body
  body="$(section_body 'Ownership fence (Issue #544)')"
  assert_states "$body" 'NEAT-AI-Backpropagation' 'which repository owns the epoch loop'
  assert_states "$body" 'trainDir' 'that the trainDir epoch loop is out of scope'
  assert_states "$body" '[Jj]ournal' 'that the journal is out of scope'
  assert_states "$body" 'traceStore' 'that the traceStore layout is out of scope'
  assert_states "$body" 'sample-rate' 'that memetic sample-rate policy is out of scope'
  assert_states "$body" 'CLI apply policy' 'that the CLI apply policy is out of scope'
}

@test "the fence points at the gates that pin it" {
  local body
  body="$(section_body 'Ownership fence (Issue #544)')"
  assert_states "$body" 'core_ownership_fence\.bats' 'which gate sweeps the sources'
  assert_states "$body" 'backprop_ffi_surface\.rs' 'which test pins the kept propagate surface'
}

@test "README defers the fence to AGENTS.md via a link" {
  run grep -q 'AGENTS.md#ownership-fence-issue-544' "$README"
  [ "$status" -eq 0 ]
}
