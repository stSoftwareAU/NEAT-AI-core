#!/usr/bin/env bats
# Tests that README.md is headed by the repo's brand banner, hot-linked from
# the NEAT-AI hub rather than copied into this repo (Issue #543).
#
# Rationale: the hub (stSoftwareAU/NEAT-AI#3764) owns the artwork and
# regenerates each per-repo preview in place at the same committed path. The
# siblings pull, they do not copy — so this README must reference the hub's raw
# URL and this repo must stay free of brand image binaries. That way a hub
# refresh (e.g. the transparent regeneration) propagates here with no PR.
#
# These are "what" tests: they read the published README artefact and assert on
# the observable outcome (a banner is present, it points at the hub, no image
# was vendored), matching the style of readme_glossary.bats.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
  BANNER_URL="https://raw.githubusercontent.com/stSoftwareAU/NEAT-AI/Develop/docs/brand/social-previews/neat-ai-core.png"
}

# Emit the parser shared by the banner assertions: it returns the (src, alt)
# of every image in the README header block — the lines from the H1 up to the
# first following heading — covering both Markdown and inline-HTML images.
header_images_py() {
  cat <<'PY'
import re
import sys


def header_images(path):
    lines = open(path, encoding="utf-8").read().splitlines()
    start = next(i for i, line in enumerate(lines) if line.startswith("# "))
    end = next(
        (
            i
            for i, line in enumerate(lines[start + 1 :], start + 1)
            if line.startswith("#")
        ),
        len(lines),
    )
    block = "\n".join(lines[start + 1 : end])
    images = [
        (m.group("src"), m.group("alt"))
        for m in re.finditer(r"!\[(?P<alt>[^\]]*)\]\((?P<src>[^)\s]+)", block)
    ]
    for tag in re.finditer(r"<img\b[^>]*>", block):
        src = re.search(r'src="([^"]*)"', tag.group(0))
        alt = re.search(r'alt="([^"]*)"', tag.group(0))
        images.append((src.group(1) if src else "", alt.group(1) if alt else ""))
    return images


def fail(message):
    sys.stderr.write(message + "\n")
    sys.exit(1)
PY
}

@test "README.md exists" {
  [ -f "$README" ]
}

@test "README.md header block carries a banner image" {
  [ -f "$README" ]
  run python3 - "$README" <<PY
$(header_images_py)

images = header_images(sys.argv[1])
if not images:
    fail("no banner image found between the H1 and the next heading")
PY
  [ "$status" -eq 0 ]
}

@test "the banner hot-links the hub's raw neat-ai-core.png" {
  [ -f "$README" ]
  run python3 - "$README" "$BANNER_URL" <<PY
$(header_images_py)

path, wanted = sys.argv[1], sys.argv[2]
sources = [src for src, _ in header_images(path)]
if wanted not in sources:
    fail(f"banner does not hot-link {wanted}; header images are {sources}")
PY
  [ "$status" -eq 0 ]
}

@test "the banner carries non-empty alt text naming the repo" {
  [ -f "$README" ]
  run python3 - "$README" "$BANNER_URL" <<PY
$(header_images_py)

path, wanted = sys.argv[1], sys.argv[2]
alts = [alt for src, alt in header_images(path) if src == wanted]
if not alts:
    fail(f"no header image with src {wanted}")
if not any("neat-ai-core" in alt.strip().lower() for alt in alts):
    fail(f"banner alt text does not name the repo: {alts}")
PY
  [ "$status" -eq 0 ]
}

@test "the banner is not a local path — no brand image is vendored" {
  [ -f "$README" ]
  run python3 - "$README" <<PY
$(header_images_py)

for src, _ in header_images(sys.argv[1]):
    if "neat-ai-core.png" in src and not src.startswith("https://"):
        fail(f"banner points at a local copy rather than the hub: {src}")
PY
  [ "$status" -eq 0 ]
}

@test "no brand image binary is committed to this repo" {
  # Siblings pull the artwork from the hub; committing a copy would fork the
  # brand and defeat the hub's in-place regeneration.
  run git -C "$REPO_ROOT" ls-files -- '*.png' '*.jpg' '*.jpeg' '*.svg' 'docs/brand'
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}
