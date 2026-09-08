#!/usr/bin/env bats
# Issue #605 — SIMD `unsafe`-block SAFETY coverage, swept over the **live**
# sources.
#
# `unsafe_simd_invariants.bats` pins the AGENTS.md / SECURITY.md *prose*; it
# never reads a line of Rust, so an `unsafe` block added with no discharge note
# fails nothing. This suite closes that hole: it compiles one definition of each
# pattern (AGENTS.md oracle rule 4) and applies it to
# `neat-core/src/simd_native.rs`, `neat-core/src/simd.rs` and
# `neat-core/src/simd/scalar.rs` **and** to good/bad literals, so gutting the
# pattern is caught by the literals and gutting a source is caught by the sweep.
#
# The rule the sweep enforces — the AGENTS.md "Unsafe & SIMD invariants" rule:
#
#   1. A block inside an `unsafe fn` carrying a `# Safety` doc is covered by
#      that function-level contract.
#   2. Any other block needs its own `// SAFETY:` note.
#   3. A block in a **safe** fn reached under an `is_*_feature_detected!` guard
#      must have a note that names that guard — it is what discharges the
#      callee's `#[target_feature]` precondition.
#   4. Every `unsafe fn` carries a `# Safety` doc, whether or not its blocks
#      happen to have notes of their own — deleting the contract must be red.
#   5. When a block in a **safe** fn calls a `#[target_feature]` fn declared in
#      the same file, an `is_*_feature_detected!` check must stand between the
#      enclosing fn's declaration and the block for **every** feature that
#      `#[target_feature]` list enables. An `avx2`-only guard in front of a
#      kernel that also enables `fma` is the Issue #605 fault, and deleting the
#      guard outright must not silence rule 3 — this is what catches it.

setup() {
  load helpers
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS_MD="${REPO_ROOT}/AGENTS.md"
  SIMD_NATIVE="${REPO_ROOT}/neat-core/src/simd_native.rs"
  SIMD_WASM="${REPO_ROOT}/neat-core/src/simd.rs"
  SIMD_SCALAR="${REPO_ROOT}/neat-core/src/simd/scalar.rs"

  # --- the one definition of each pattern (AGENTS.md oracle rule 4) ---------
  # Exported, never re-spelled: the live sweep and the good/bad literal checks
  # below both compile exactly these.
  export UNSAFE_BLOCK_RE='(?<![A-Za-z0-9_])unsafe[ \t]*\{'
  export SAFETY_NOTE_RE='^[ \t]*//[ \t]*SAFETY:'
  export SAFETY_DOC_RE='^[ \t]*///[ \t]*#+[ \t]*Safety\b'
  export FN_DECL_RE='^(?P<indent>[ \t]*)(?:pub(?:\([^)]*\))?[ \t]+)?(?:const[ \t]+)?(?P<unsafe>unsafe[ \t]+)?(?:extern[ \t]+"[^"]*"[ \t]+)?fn[ \t]+(?P<name>[A-Za-z_][A-Za-z0-9_]*)'
  export FEATURE_GUARD_RE='is_(x86|aarch64)_feature_detected!'
  # The feature a guard proves, and the features a `#[target_feature]` list
  # enables: rule 5 compares one set against the other.
  export GUARDED_FEATURE_RE='is_(?:x86|aarch64)_feature_detected!\([ \t]*"(?P<feature>[^"]+)"'
  export TARGET_FEATURE_RE='^[ \t]*#\[target_feature\((?P<features>[^)]*)\)\]'
  export ENABLE_RE='enable[ \t]*=[ \t]*"(?P<feature>[^"]+)"'
  export CALL_RE='(?<![A-Za-z0-9_])(?P<name>[A-Za-z_][A-Za-z0-9_]*)[ \t]*(?:::<[^>]*>[ \t]*)?\('
  # A `// SAFETY:` note reaches a block through comment continuation lines and
  # through neighbouring statements that themselves open an `unsafe` block, so
  # one note may cover a contiguous run of them.
  export CARRY_RE='^[ \t]*(//|.*(?<![A-Za-z0-9_])unsafe[ \t]*\{)'
  export NOTE_WINDOW=8

  WORK="$(mktemp -d)"
  CHECKER="${WORK}/unsafe_block_safety_notes.py"
  write_checker "$CHECKER"
}

teardown() {
  if [ -n "${WORK:-}" ]; then
    rm -rf "$WORK"
  fi
}

# The sweep itself. Quoted heredoc (`<<'PY'`) with `os.environ[...]`: the shell
# must not interpolate or re-escape a single character of the patterns above.
write_checker() {
  cat >"$1" <<'PY'
import os
import re
import sys

UNSAFE_BLOCK_RE = re.compile(os.environ["UNSAFE_BLOCK_RE"])
SAFETY_NOTE_RE = re.compile(os.environ["SAFETY_NOTE_RE"])
SAFETY_DOC_RE = re.compile(os.environ["SAFETY_DOC_RE"])
FN_DECL_RE = re.compile(os.environ["FN_DECL_RE"])
FEATURE_GUARD_RE = re.compile(os.environ["FEATURE_GUARD_RE"])
CARRY_RE = re.compile(os.environ["CARRY_RE"])
GUARDED_FEATURE_RE = re.compile(os.environ["GUARDED_FEATURE_RE"])
TARGET_FEATURE_RE = re.compile(os.environ["TARGET_FEATURE_RE"])
ENABLE_RE = re.compile(os.environ["ENABLE_RE"])
CALL_RE = re.compile(os.environ["CALL_RE"])
NOTE_WINDOW = int(os.environ["NOTE_WINDOW"])


def code_part(line):
    """The code half of `line` — everything before a `//` comment starts."""
    cut = line.find("//")
    return line if cut < 0 else line[:cut]


def enclosing_fn(lines, idx):
    """The nearest `fn` declaration above `idx` that is less indented than it."""
    indent = len(lines[idx]) - len(lines[idx].lstrip())
    for j in range(idx - 1, -1, -1):
        m = FN_DECL_RE.match(lines[j])
        if m and len(m.group("indent")) < indent:
            return j, m
    return None, None


def has_safety_doc(lines, decl):
    """A `# Safety` heading in the doc/attribute block right above `decl`."""
    for j in range(decl - 1, -1, -1):
        stripped = lines[j].strip()
        if not (stripped.startswith("//") or stripped.startswith("#[")):
            return False
        if SAFETY_DOC_RE.match(lines[j]):
            return True
    return False


def covering_note(lines, idx):
    """Index of the `// SAFETY:` note covering the block at `idx`, else None."""
    for j in range(idx - 1, max(idx - 1 - NOTE_WINDOW, -1), -1):
        if SAFETY_NOTE_RE.match(lines[j]):
            return j
        if not CARRY_RE.match(lines[j]):
            return None
    return None


def note_text(lines, note, idx):
    """The full comment run starting at `note` and reaching the block."""
    out = ""
    k = note
    while k < idx and lines[k].strip().startswith("//"):
        out += lines[k]
        k += 1
    return out


def guarded_by_feature_detection(lines, decl, idx):
    # `code_part` matters: a note *mentioning* the guard is not the guard.
    return any(FEATURE_GUARD_RE.search(code_part(line)) for line in lines[decl:idx])


def detected_features(lines, decl, idx):
    """Features an `is_*_feature_detected!` check proves between `decl` and the block.

    Comments are stripped first — a `// SAFETY:` note that names the guard is
    documentation, not a runtime check, and must not stand in for one.
    """
    found = set()
    for line in lines[decl:idx]:
        for m in GUARDED_FEATURE_RE.finditer(code_part(line)):
            found.add(m.group("feature"))
    return found


def enabled_features(lines, decl):
    """Features the `#[target_feature]` list above `decl` enables."""
    found = set()
    for j in range(decl - 1, -1, -1):
        stripped = lines[j].strip()
        if not (stripped.startswith("//") or stripped.startswith("#[")):
            break
        m = TARGET_FEATURE_RE.match(lines[j])
        if m:
            for enable in ENABLE_RE.finditer(m.group("features")):
                found.add(enable.group("feature"))
    return found


def target_feature_fns(lines):
    """Map every `#[target_feature]` fn declared in the file to the features it enables."""
    table = {}
    for j, line in enumerate(lines):
        m = FN_DECL_RE.match(line)
        if not m:
            continue
        features = enabled_features(lines, j)
        if features:
            table[m.group("name")] = features
    return table


def called_names(lines, idx):
    """Function names called by the block at `idx` and the lines it spans."""
    names = set()
    depth = 0
    started = False
    for line in lines[idx:]:
        code = code_part(line)
        for m in CALL_RE.finditer(code):
            names.add(m.group("name"))
        depth += code.count("{") - code.count("}")
        started = started or "{" in code
        if started and depth <= 0:
            break
    return names


def unsafe_fns_missing_safety_doc(lines):
    """Yield (line_no, name) for every `unsafe fn` with no `# Safety` doc."""
    for j, line in enumerate(lines):
        m = FN_DECL_RE.match(line)
        if m and m.group("unsafe") and not has_safety_doc(lines, j):
            yield j + 1, m.group("name")


def safety_doc_lines(lines):
    """Yield the `# Safety` heading line of every `unsafe fn` in the file.

    These are the contracts rule 4 makes load-bearing; a `# Safety` doc on a
    *safe* fn is not one of them.
    """
    for j, line in enumerate(lines):
        m = FN_DECL_RE.match(line)
        if not (m and m.group("unsafe")):
            continue
        for k in range(j - 1, -1, -1):
            stripped = lines[k].strip()
            if not (stripped.startswith("//") or stripped.startswith("#[")):
                break
            if SAFETY_DOC_RE.match(lines[k]):
                yield k + 1
                break


def sweep(path):
    """Yield (line_no, verdict, detail) for every `unsafe {` block in `path`."""
    with open(path, encoding="utf-8") as handle:
        lines = handle.read().splitlines()
    kernels = target_feature_fns(lines)
    for line_no, name in unsafe_fns_missing_safety_doc(lines):
        yield line_no, "no-safety-doc", (
            "`unsafe fn %s` states no `# Safety` contract for its caller" % name
        )
    for i, line in enumerate(lines):
        if not UNSAFE_BLOCK_RE.search(code_part(line)):
            continue
        decl, decl_match = enclosing_fn(lines, i)
        if decl is not None and decl_match.group("unsafe") and has_safety_doc(lines, decl):
            yield i + 1, "ok", (
                "`# Safety` doc on `unsafe fn %s`" % decl_match.group("name")
            )
            continue
        note = covering_note(lines, i)
        if note is None:
            where = "fn %s" % decl_match.group("name") if decl_match else "module scope"
            yield i + 1, "uncovered", (
                "no `// SAFETY:` note, and no `# Safety` doc on an enclosing "
                "`unsafe fn` (%s)" % where
            )
            continue
        if decl is not None and guarded_by_feature_detection(lines, decl, i):
            if not FEATURE_GUARD_RE.search(note_text(lines, note, i)):
                yield i + 1, "unnamed-guard", (
                    "`// SAFETY:` note does not name the `is_*_feature_detected!` "
                    "guard that discharges the `#[target_feature]` precondition"
                )
                continue
        if decl is not None:
            # Rule 5: the guard must detect every feature the callee enables,
            # unless the enclosing fn already enables them itself.
            detected = detected_features(lines, decl, i) | enabled_features(lines, decl)
            for name in sorted(called_names(lines, i)):
                required = kernels.get(name)
                if required and not required <= detected:
                    missing = "/".join(sorted(required - detected))
                    seen = "/".join(sorted(detected)) or "no feature"
                    yield i + 1, "unguarded-feature", (
                        "calls `%s`, which enables %s, but only %s is detected "
                        "before this block — %s is unguarded" % (
                            name, "/".join(sorted(required)), seen, missing
                        )
                    )
                    break
            else:
                yield i + 1, "ok", "`// SAFETY:` note at line %d" % (note + 1)
            continue
        yield i + 1, "ok", "`// SAFETY:` note at line %d" % (note + 1)


def main(argv):
    report = "--report" in argv
    paths = [a for a in argv if not a.startswith("--")]
    if not paths:
        sys.exit("no source files given — a sweep over nothing is not a pass")
    if "--safety-docs" in argv:
        for path in paths:
            with open(path, encoding="utf-8") as handle:
                for line_no in safety_doc_lines(handle.read().splitlines()):
                    print(line_no)
        return 0
    failures = 0
    blocks = 0
    for path in paths:
        for line_no, verdict, detail in sweep(path):
            blocks += 1
            if verdict != "ok":
                failures += 1
            if report or verdict != "ok":
                print("%s:%d: %s: %s" % (path, line_no, verdict, detail))
    if not blocks:
        sys.exit("swept %d file(s) and found no `unsafe {` block at all" % len(paths))
    print("swept %d unsafe block(s), %d uncovered" % (blocks, failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
PY
}

# Drop line `$2` of file `$1` — the mutation the coverage tests below apply.
drop_line() {
  awk -v n="$2" 'NR != n' "$1" >"${1}.mutant" && mv "${1}.mutant" "$1"
}

# --- the live sweep -------------------------------------------------------

@test "the three SIMD sources this gate sweeps all exist" {
  [ -f "$SIMD_NATIVE" ]
  [ -f "$SIMD_WASM" ]
  [ -f "$SIMD_SCALAR" ]
}

@test "every unsafe block in the live SIMD sources carries the agreed SAFETY coverage" {
  require_python3
  run python3 "$CHECKER" "$SIMD_NATIVE" "$SIMD_WASM" "$SIMD_SCALAR"
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "the live sweep reaches every one of the three sources" {
  require_python3
  for source in "$SIMD_NATIVE" "$SIMD_WASM" "$SIMD_SCALAR"; do
    run python3 "$CHECKER" --report "$source"
    echo "$output"
    [ "$status" -eq 0 ]
    # Fail loud rather than passing vacuously on a file the sweep never entered.
    [[ "$output" != *"found no \`unsafe {\` block at all"* ]]
  done
}

# --- mutation evidence: the sweep can fail on the live sources ------------

@test "deleting any load-bearing SAFETY note from a live source turns the sweep red" {
  require_python3
  # Exhaustive, not one hand-picked line: every note the sweep reports as the
  # thing covering a block must be load-bearing. (A note inside an `unsafe fn`
  # is *not* in this set — rule 1 covers those blocks, which is the whole point
  # of the convention.)
  for source in "$SIMD_NATIVE" "$SIMD_WASM" "$SIMD_SCALAR"; do
    note_lines="$(python3 "$CHECKER" --report "$source" |
      sed -n 's/.*`\/\/ SAFETY:` note at line \([0-9][0-9]*\)$/\1/p' | sort -run)"
    for note_line in $note_lines; do
      cp "$source" "${WORK}/mutant.rs"
      drop_line "${WORK}/mutant.rs" "$note_line"
      run python3 "$CHECKER" "${WORK}/mutant.rs"
      echo "deleted ${source}:${note_line} — $output"
      [ "$status" -eq 1 ]
    done
  done
}

@test "deleting any # Safety doc from a live unsafe fn turns the sweep red" {
  require_python3
  # Also exhaustive, over the contracts rule 4 makes load-bearing: an
  # `unsafe fn` without its `# Safety` doc is red even when every block in its
  # body happens to carry a note of its own.
  for source in "$SIMD_NATIVE" "$SIMD_WASM" "$SIMD_SCALAR"; do
    doc_lines="$(python3 "$CHECKER" --safety-docs "$source" | sort -run)"
    for doc_line in $doc_lines; do
      cp "$source" "${WORK}/mutant.rs"
      drop_line "${WORK}/mutant.rs" "$doc_line"
      run python3 "$CHECKER" "${WORK}/mutant.rs"
      echo "deleted ${source}:${doc_line} — $output"
      [ "$status" -eq 1 ]
    done
  done
}

@test "weakening a dispatch guard to one of the kernel's two features turns the sweep red" {
  require_python3
  cp "$SIMD_NATIVE" "${WORK}/mutant.rs"
  # The Issue #605 fault itself: the AVX2 kernels enable avx2 + fma, so dropping
  # the fma detection leaves an FMA intrinsic reachable on an AVX2-only CPU.
  fma_line="$(grep -n 'is_x86_feature_detected!("fma"),' "${WORK}/mutant.rs" | head -1 | cut -d: -f1)"
  [ -n "$fma_line" ]
  drop_line "${WORK}/mutant.rs" "$fma_line"
  run python3 "$CHECKER" "${WORK}/mutant.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"unguarded-feature"* ]]
}

# --- good literals: the same pattern must accept these --------------------

@test "a # Safety doc on an unsafe fn covers the unsafe blocks in its body" {
  require_python3
  cat >"${WORK}/good_unsafe_fn.rs" <<'RS'
    /// # Safety
    /// Caller must ensure AVX2 and FMA are enabled.
    #[target_feature(enable = "avx2", enable = "fma")]
    #[inline]
    pub unsafe fn kernel(xs: &[f32], i: usize) -> f32 {
        let a = unsafe { *xs.get_unchecked(i) };
        unsafe { *xs.get_unchecked(i + 1) } + a
    }
RS
  run python3 "$CHECKER" "${WORK}/good_unsafe_fn.rs"
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "a per-block note naming the feature guard is accepted in a safe fn" {
  require_python3
  cat >"${WORK}/good_dispatch.rs" <<'RS'
    pub fn dispatch(xs: &[f32], i: usize) -> f32 {
        if std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: the `is_x86_feature_detected!("fma")` guard above proves
            // the `#[target_feature(enable = "fma")]` precondition holds.
            return unsafe { kernel(xs, i) };
        }
        scalar(xs, i)
    }
RS
  run python3 "$CHECKER" "${WORK}/good_dispatch.rs"
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "one note covers a contiguous run of unsafe blocks" {
  require_python3
  cat >"${WORK}/good_run.rs" <<'RS'
    pub fn tail(xs: &[f32], ss: &[u16], i: usize) -> f32 {
        // SAFETY: `i` is in range for both slices by the load-time index
        // validation in `CompiledNetwork::new`.
        let s = unsafe { *ss.get_unchecked(i) };
        unsafe { *xs.get_unchecked(s as usize) }
    }
RS
  run python3 "$CHECKER" "${WORK}/good_run.rs"
  echo "$output"
  [ "$status" -eq 0 ]
}

# --- bad literals: the same pattern must reject these ---------------------

@test "an unsafe block in a safe fn with no note is rejected" {
  require_python3
  cat >"${WORK}/bad_no_note.rs" <<'RS'
    pub fn dispatch(xs: &[f32], i: usize) -> f32 {
        unsafe { *xs.get_unchecked(i) }
    }
RS
  run python3 "$CHECKER" "${WORK}/bad_no_note.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"uncovered"* ]]
}

@test "an unsafe fn with no # Safety doc does not cover its blocks" {
  require_python3
  cat >"${WORK}/bad_no_doc.rs" <<'RS'
    /// Undocumented contract.
    #[target_feature(enable = "avx2")]
    pub unsafe fn kernel(xs: &[f32], i: usize) -> f32 {
        unsafe { *xs.get_unchecked(i) }
    }
RS
  run python3 "$CHECKER" "${WORK}/bad_no_doc.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"uncovered"* ]]
}

@test "a note that does not name the feature guard is rejected in a safe fn" {
  require_python3
  cat >"${WORK}/bad_unnamed_guard.rs" <<'RS'
    pub fn dispatch(xs: &[f32], i: usize) -> f32 {
        if std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: the FMA guard proves the target-feature precondition.
            return unsafe { kernel(xs, i) };
        }
        scalar(xs, i)
    }
RS
  run python3 "$CHECKER" "${WORK}/bad_unnamed_guard.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"unnamed-guard"* ]]
}

@test "an unsafe fn with no # Safety doc is rejected even when its blocks are annotated" {
  require_python3
  cat >"${WORK}/bad_annotated_no_doc.rs" <<'RS'
    /// Fast path. No contract stated for the caller.
    #[target_feature(enable = "avx2")]
    pub unsafe fn kernel(xs: &[f32], i: usize) -> f32 {
        // SAFETY: `i < xs.len()` by the caller's chunk-loop bound.
        unsafe { *xs.get_unchecked(i) }
    }
RS
  run python3 "$CHECKER" "${WORK}/bad_annotated_no_doc.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"no-safety-doc"* ]]
}

@test "a guard that detects only one of the callee's two features is rejected" {
  require_python3
  cat >"${WORK}/bad_partial_guard.rs" <<'RS'
    /// # Safety
    /// Caller must ensure AVX2 and FMA are enabled.
    #[target_feature(enable = "avx2", enable = "fma")]
    pub unsafe fn kernel(xs: &[f32], i: usize) -> f32 {
        unsafe { *xs.get_unchecked(i) }
    }

    pub fn dispatch(xs: &[f32], i: usize) -> f32 {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the `is_x86_feature_detected!("avx2")` guard above proves
            // the `#[target_feature]` precondition holds.
            return unsafe { kernel(xs, i) };
        }
        scalar(xs, i)
    }
RS
  run python3 "$CHECKER" "${WORK}/bad_partial_guard.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"unguarded-feature"* ]]
}

@test "a guard that detects both of the callee's features is accepted" {
  require_python3
  cat >"${WORK}/good_full_guard.rs" <<'RS'
    /// # Safety
    /// Caller must ensure AVX2 and FMA are enabled.
    #[target_feature(enable = "avx2", enable = "fma")]
    pub unsafe fn kernel(xs: &[f32], i: usize) -> f32 {
        unsafe { *xs.get_unchecked(i) }
    }

    pub fn dispatch(xs: &[f32], i: usize) -> f32 {
        if std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
        {
            // SAFETY: the `is_x86_feature_detected!("avx2")` and `("fma")` guards
            // above prove the `#[target_feature]` precondition holds.
            return unsafe { kernel(xs, i) };
        }
        scalar(xs, i)
    }
RS
  run python3 "$CHECKER" "${WORK}/good_full_guard.rs"
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "a note naming the guard does not stand in for the guard itself" {
  require_python3
  # The note text mentions `is_x86_feature_detected!("fma")` but no such check
  # runs — comments must be stripped before the guard set is computed.
  cat >"${WORK}/bad_note_only_guard.rs" <<'RS'
    /// # Safety
    /// Caller must ensure FMA is enabled.
    #[target_feature(enable = "fma")]
    pub unsafe fn kernel(xs: &[f32], i: usize) -> f32 {
        unsafe { *xs.get_unchecked(i) }
    }

    pub fn dispatch(xs: &[f32], i: usize) -> f32 {
        // SAFETY: the `is_x86_feature_detected!("fma")` guard proves the
        // `#[target_feature(enable = "fma")]` precondition holds.
        unsafe { kernel(xs, i) }
    }
RS
  run python3 "$CHECKER" "${WORK}/bad_note_only_guard.rs"
  echo "$output"
  [ "$status" -eq 1 ]
  [[ "$output" == *"unguarded-feature"* ]]
}

@test "a sweep over no source at all fails loud rather than passing" {
  require_python3
  run python3 "$CHECKER"
  [ "$status" -ne 0 ]
}
