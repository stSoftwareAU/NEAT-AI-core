"""Vendored YAML subset parser — the stand-in used when PyYAML is missing.

Issue #642: the `tests/scripts` bats suite reads GitHub workflow YAML through
`python3 … import yaml`, and a host whose python3 has no PyYAML (the unattended
worker container) failed 111 of 507 tests rather than running them, so
`./quality.sh` never reached its TypeScript, Mermaid, Deno or Rust stages
locally. `tests/scripts/helpers.bash` puts this directory on `PYTHONPATH` only
when PyYAML is genuinely unavailable, so those assertions keep being *made*
rather than skipped; where PyYAML exists it is still what parses the workflows.

Scope — the subset GitHub workflow, dependabot and issue-template YAML uses:

* block mappings and block sequences, nested to any depth;
* flow sequences (``[a, b]``) and flow mappings (``{a: b}``) on one line;
* plain, single-quoted and double-quoted scalars;
* literal (``|``) and folded (``>``) block scalars, with the ``-``/``+``
  chomping and explicit-indentation indicators;
* comments, including a trailing comment after a value;
* the YAML 1.1 plain-scalar resolution PyYAML applies, so ``on:`` is the key
  ``True``, ``false`` is a bool and ``20`` is an int.

Everything outside that subset — anchors, aliases, tags, multiple documents,
complex keys, multi-line plain scalars — raises `YAMLError` rather than being
guessed at, because a gate that mis-parses a workflow is worse than one that
stops. `tests/scripts/yaml_fallback.bats` pins the parser against PyYAML over
every YAML file in the repository, so a divergence fails the suite.
"""

import re

__all__ = ["YAMLError", "safe_load", "safe_dump"]


class YAMLError(Exception):
    """Malformed input, or input outside the supported subset."""


# --- plain scalar resolution (YAML 1.1, as PyYAML implements it) -------------

_NULLS = {"", "~", "null", "Null", "NULL"}

_BOOLS = {
    "yes": True, "Yes": True, "YES": True,
    "no": False, "No": False, "NO": False,
    "true": True, "True": True, "TRUE": True,
    "false": False, "False": False, "FALSE": False,
    "on": True, "On": True, "ON": True,
    "off": False, "Off": False, "OFF": False,
}

_INT_RE = re.compile(
    r"""^(?:[-+]?0b[0-1_]+
         |[-+]?0[0-7_]+
         |[-+]?(?:0|[1-9][0-9_]*)
         |[-+]?0x[0-9a-fA-F_]+
         |[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$""",
    re.X,
)

_FLOAT_RE = re.compile(
    r"""^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?
         |\.[0-9_]+(?:[eE][-+][0-9]+)?
         |[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
         |[-+]?\.(?:inf|Inf|INF)
         |\.(?:nan|NaN|NAN))$""",
    re.X,
)

_BLOCK_HEADER_RE = re.compile(r"^([|>])([0-9]*)([-+]?)([0-9]*)\s*(#.*)?$")


def _resolve_int(text):
    body = text.replace("_", "")
    sign = 1
    if body[:1] in "+-":
        sign = -1 if body[0] == "-" else 1
        body = body[1:]
    if body.startswith("0b"):
        return sign * int(body[2:], 2)
    if body.startswith("0x"):
        return sign * int(body[2:], 16)
    if ":" in body:
        total = 0
        for part in body.split(":"):
            total = total * 60 + int(part)
        return sign * total
    if body.startswith("0") and len(body) > 1:
        return sign * int(body, 8)
    return sign * int(body)


def _resolve_float(text):
    body = text.replace("_", "")
    if ":" in body:
        sign = 1
        if body[:1] in "+-":
            sign = -1 if body[0] == "-" else 1
            body = body[1:]
        head, _, tail = body.rpartition(":")
        total = 0.0
        for part in head.split(":"):
            total = total * 60 + int(part)
        return sign * (total * 60 + float(tail))
    return float(body.replace(".inf", "1e999").replace(".Inf", "1e999")
                 .replace(".INF", "1e999"))


def resolve(text):
    """Resolve a plain scalar to the Python value PyYAML would produce."""
    if text in _NULLS:
        return None
    if text in _BOOLS:
        return _BOOLS[text]
    if _INT_RE.match(text):
        return _resolve_int(text)
    if _FLOAT_RE.match(text):
        return _resolve_float(text)
    return text


# --- scalar scanning ---------------------------------------------------------

_ESCAPES = {
    "0": "\0", "a": "\a", "b": "\b", "t": "\t", "\t": "\t", "n": "\n",
    "v": "\v", "f": "\f", "r": "\r", "e": "\x1b", " ": " ", '"': '"',
    "/": "/", "\\": "\\", "N": "\x85", "_": "\xa0",
}


def _scan_quoted(text, i):
    """Scan the quoted scalar starting at text[i]; return (value, end index)."""
    quote = text[i]
    i += 1
    out = []
    while i < len(text):
        char = text[i]
        if quote == "'":
            if char == "'":
                if text[i + 1:i + 2] == "'":
                    out.append("'")
                    i += 2
                    continue
                return "".join(out), i + 1
            out.append(char)
            i += 1
            continue
        if char == "\\":
            code = text[i + 1:i + 2]
            if code in ("x", "u", "U"):
                width = {"x": 2, "u": 4, "U": 8}[code]
                digits = text[i + 2:i + 2 + width]
                if len(digits) != width:
                    raise YAMLError("truncated escape in %r" % text)
                out.append(chr(int(digits, 16)))
                i += 2 + width
                continue
            if code not in _ESCAPES:
                raise YAMLError("unknown escape %r in %r" % (code, text))
            out.append(_ESCAPES[code])
            i += 2
            continue
        if char == '"':
            return "".join(out), i + 1
        out.append(char)
        i += 1
    raise YAMLError("unterminated quoted scalar: %r" % text)


def _strip_comment(text):
    """Drop a trailing `#` comment — one starting the text or preceded by space."""
    i = 0
    while True:
        found = text.find("#", i)
        if found == -1:
            return text
        if found == 0 or text[found - 1] in " \t":
            return text[:found]
        i = found + 1


def _scan_flow(text, i):
    """Scan the flow collection starting at text[i]; return (value, end index)."""
    opener = text[i]
    closer = "]" if opener == "[" else "}"
    is_map = opener == "{"
    i += 1
    result = {} if is_map else []
    while True:
        i = _skip_spaces(text, i)
        if i >= len(text):
            raise YAMLError("unterminated flow collection: %r" % text)
        if text[i] == closer:
            return result, i + 1
        item, i = _scan_flow_scalar(text, i)
        i = _skip_spaces(text, i)
        if is_map:
            if i >= len(text) or text[i] != ":":
                raise YAMLError("flow mapping entry has no value: %r" % text)
            value, i = _scan_flow_scalar(text, _skip_spaces(text, i + 1))
            result[item] = value
            i = _skip_spaces(text, i)
        else:
            result.append(item)
        if i < len(text) and text[i] == ",":
            i += 1
            continue
        if i < len(text) and text[i] == closer:
            return result, i + 1
        raise YAMLError("expected ',' or %r in flow collection: %r" % (closer, text))


def _skip_spaces(text, i):
    while i < len(text) and text[i] in " \t":
        i += 1
    return i


def _scan_flow_scalar(text, i):
    """Scan one element inside a flow collection; return (value, end index)."""
    if text[i] in "\"'":
        return _scan_quoted(text, i)
    if text[i] in "[{":
        return _scan_flow(text, i)
    if text[i] in "&*!":
        raise YAMLError("anchors, aliases and tags are outside the supported "
                        "subset: %r" % text)
    start = i
    while i < len(text) and text[i] not in ",[]{}:":
        i += 1
    return resolve(text[start:i].strip()), i


def _parse_value_text(text):
    """Parse the scalar or flow collection written after a `key:` or `-`."""
    text = text.strip()
    if text == "" or text.startswith("#"):
        return None
    if text[0] in "\"'":
        value, end = _scan_quoted(text, 0)
    elif text[0] in "[{":
        value, end = _scan_flow(text, 0)
    elif text[0] in "&*!":
        raise YAMLError("anchors, aliases and tags are outside the supported "
                        "subset: %r" % text)
    else:
        return resolve(_strip_comment(text).strip())
    trailing = text[end:].strip()
    if trailing and not trailing.startswith("#"):
        raise YAMLError("trailing content after value: %r" % text)
    return value


def _split_key(text):
    """Split `key: value` into (key text, rest), or return None when it is not one."""
    i = 0
    if text[0] in "\"'":
        _, i = _scan_quoted(text, 0)
        key_text = text[:i]
    else:
        while i < len(text):
            if text[i] == ":" and (i + 1 == len(text) or text[i + 1] in " \t"):
                break
            i += 1
        if i >= len(text):
            return None
        key_text = text[:i]
    rest = text[i:]
    if not rest.startswith(":"):
        rest = rest.lstrip()
        if not rest.startswith(":"):
            return None
    return key_text.strip(), rest[1:]


def _parse_key(key_text):
    if key_text[:1] in "\"'":
        value, end = _scan_quoted(key_text, 0)
        if key_text[end:].strip():
            raise YAMLError("trailing content after quoted key: %r" % key_text)
        return value
    if key_text[:1] in "[{":
        raise YAMLError("complex keys are outside the supported subset: %r"
                        % key_text)
    return resolve(_strip_comment(key_text).strip())


# --- block structure ---------------------------------------------------------

class _Line(object):
    __slots__ = ("indent", "text", "raw", "number")

    def __init__(self, indent, text, raw, number):
        self.indent = indent
        self.text = text
        self.raw = raw
        self.number = number

    @property
    def blank(self):
        return self.text == "" or self.text.startswith("#")


def _scan_lines(text):
    lines = []
    for number, raw in enumerate(text.splitlines(), start=1):
        if "\t" in raw[:len(raw) - len(raw.lstrip(" \t"))]:
            raise YAMLError("line %d: tabs cannot be used for indentation" % number)
        stripped = raw.lstrip(" ")
        lines.append(_Line(len(raw) - len(stripped), stripped.rstrip(), raw, number))
    return lines


class _Parser(object):
    def __init__(self, lines):
        self.lines = lines

    def next_significant(self, i):
        while i < len(self.lines) and self.lines[i].blank:
            i += 1
        return i

    def parse_document(self):
        i = self.next_significant(0)
        if i < len(self.lines) and self.lines[i].text == "---":
            i = self.next_significant(i + 1)
        if i >= len(self.lines):
            return None
        value, i = self.parse_node(i, self.lines[i].indent)
        i = self.next_significant(i)
        if i < len(self.lines) and self.lines[i].text in ("...", "---"):
            raise YAMLError("line %d: multiple documents are outside the "
                            "supported subset" % self.lines[i].number)
        if i < len(self.lines):
            raise YAMLError("line %d: unexpected content %r"
                            % (self.lines[i].number, self.lines[i].text))
        return value

    def parse_node(self, i, indent):
        line = self.lines[i]
        if line.text == "-" or line.text.startswith("- "):
            return self.parse_sequence(i, indent)
        return self.parse_mapping(i, indent)

    def parse_mapping(self, i, indent):
        result = {}
        while True:
            i = self.next_significant(i)
            if i >= len(self.lines):
                return result, i
            line = self.lines[i]
            if line.indent < indent:
                return result, i
            if line.indent > indent:
                raise YAMLError("line %d: unexpected indentation in mapping: %r"
                                % (line.number, line.text))
            if line.text == "-" or line.text.startswith("- "):
                raise YAMLError("line %d: sequence entry inside a mapping: %r"
                                % (line.number, line.text))
            split = _split_key(line.text)
            if split is None:
                raise YAMLError("line %d: expected 'key: value', got %r"
                                % (line.number, line.text))
            key_text, rest = split
            key = _parse_key(key_text)
            header = _BLOCK_HEADER_RE.match(rest.strip())
            if rest.strip() == "" or rest.strip().startswith("#"):
                value, i = self.parse_child(i, indent)
            elif header:
                value, i = self.block_scalar(i, header, indent)
            else:
                value, i = _parse_value_text(rest), i + 1
            result[key] = value

    def parse_sequence(self, i, indent):
        items = []
        while True:
            i = self.next_significant(i)
            if i >= len(self.lines):
                return items, i
            line = self.lines[i]
            if line.indent < indent:
                return items, i
            if line.indent > indent:
                raise YAMLError("line %d: unexpected indentation in sequence: %r"
                                % (line.number, line.text))
            if not (line.text == "-" or line.text.startswith("- ")):
                return items, i
            if line.text == "-":
                value, i = self.parse_child(i, indent)
                items.append(value)
                continue
            body = line.text[1:]
            offset = len(body) - len(body.lstrip(" "))
            child_indent = indent + 1 + offset
            child = _Line(child_indent, body.lstrip(" "),
                          " " * child_indent + body.lstrip(" "), line.number)
            header = _BLOCK_HEADER_RE.match(child.text)
            if header:
                # `- |` — a block scalar as the sequence entry itself.
                value, i = self.block_scalar(i, header, indent)
                items.append(value)
                continue
            self.lines[i] = child
            if _split_key(child.text) is None:
                value, i = _parse_value_text(child.text), i + 1
            else:
                value, i = self.parse_node(i, child_indent)
            items.append(value)

    def parse_child(self, i, indent):
        """Parse the nested node owned by the key or dash on line i."""
        j = self.next_significant(i + 1)
        if j >= len(self.lines):
            return None, i + 1
        child = self.lines[j]
        if child.indent > indent:
            return self.parse_node(j, child.indent)
        if child.indent == indent and (child.text == "-"
                                       or child.text.startswith("- ")):
            # A sequence may sit at its key's own indentation.
            return self.parse_sequence(j, indent)
        return None, i + 1

    def block_scalar(self, i, header, parent_indent):
        style, indent_a, chomp, indent_b, _ = header.groups()
        explicit = indent_a or indent_b
        collected = []
        j = i + 1
        while j < len(self.lines):
            line = self.lines[j]
            if line.raw.strip() == "":
                collected.append(line)
                j += 1
                continue
            if line.indent <= parent_indent:
                break
            collected.append(line)
            j += 1
        # Trailing blank lines stay in the block: `|+` keeps them, and the
        # other chomping modes strip them below, so popping them here would
        # silently turn a `keep` block into a `clip` one.
        content = [line for line in collected if line.raw.strip() != ""]
        if not content:
            return _chomp("\n" * len(collected), chomp), j
        if explicit:
            block_indent = parent_indent + int(explicit)
        else:
            block_indent = min(line.indent for line in content)
        body = []
        for line in collected:
            if line.raw.strip() == "":
                body.append("")
            else:
                if line.indent < block_indent:
                    raise YAMLError("line %d: block scalar line is less "
                                    "indented than the block" % line.number)
                body.append(line.raw[block_indent:].rstrip("\r"))
        if style == "|":
            text = "".join(part + "\n" for part in body)
        else:
            text = _fold(body)
        return _chomp(text, chomp), j


def _fold(body):
    """Fold a `>` block: a break between two plain lines becomes a space."""
    text = ""
    previous = None
    for line in body:
        if line.strip() == "":
            kind = "empty"
        elif line[:1] in (" ", "\t"):
            kind = "more"
        else:
            kind = "text"
        if previous is None:
            text = line
        elif kind == "empty":
            text += "\n"
        elif previous == "empty":
            text += line
        elif previous == "text" and kind == "text":
            text += " " + line
        else:
            text += "\n" + line
        previous = kind
    return text + "\n"


def _chomp(text, indicator):
    if indicator == "-":
        return text.rstrip("\n")
    if indicator == "+":
        return text
    stripped = text.rstrip("\n")
    return stripped + "\n" if stripped else ""


# --- public API --------------------------------------------------------------

def safe_load(stream):
    """Parse a YAML document from a string, bytes or file object."""
    if hasattr(stream, "read"):
        stream = stream.read()
    if isinstance(stream, bytes):
        stream = stream.decode("utf-8")
    return _Parser(_scan_lines(stream)).parse_document()


def _sort_key(key):
    return (0 if isinstance(key, str) else 1, str(key))


_PLAIN_SAFE_RE = re.compile(r"^[A-Za-z0-9_./@+-][A-Za-z0-9 _./@+()-]*$")


def _dump_scalar(value):
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, (int, float)):
        return repr(value)
    text = str(value)
    if _PLAIN_SAFE_RE.match(text) and not isinstance(resolve(text),
                                                     (bool, int, float)) \
            and resolve(text) is not None:
        return text
    return "'" + text.replace("'", "''") + "'"


def _dump_leaf(prefix, value, pad):
    if isinstance(value, str) and "\n" in value:
        style = "|" if value.endswith("\n") else "|-"
        body = value.rstrip("\n").split("\n")
        lines = ["%s%s %s\n" % (pad, prefix, style)]
        lines += ["%s  %s\n" % (pad, line) if line else "\n" for line in body]
        return "".join(lines)
    return "%s%s %s\n" % (pad, prefix, _dump_scalar(value))


def _dump_node(value, indent, sort_keys):
    pad = " " * indent
    if isinstance(value, dict):
        out = []
        keys = sorted(value, key=_sort_key) if sort_keys else list(value)
        for key in keys:
            item = value[key]
            name = _dump_scalar(key)
            if isinstance(item, (dict, list)) and item:
                out.append("%s%s:\n" % (pad, name))
                out.append(_dump_node(item, indent + 2, sort_keys))
            elif isinstance(item, dict):
                out.append("%s%s: {}\n" % (pad, name))
            elif isinstance(item, list):
                out.append("%s%s: []\n" % (pad, name))
            else:
                out.append(_dump_leaf(name + ":", item, pad))
        return "".join(out)
    if isinstance(value, list):
        out = []
        for item in value:
            if isinstance(item, (dict, list)) and item:
                block = _dump_node(item, indent + 2, sort_keys)
                first, _, rest = block.partition("\n")
                out.append("%s- %s\n%s" % (pad, first.strip(), rest))
            elif isinstance(item, (dict, list)):
                out.append("%s- %s\n" % (pad, "{}" if isinstance(item, dict) else "[]"))
            else:
                out.append(_dump_leaf("-", item, pad))
        return "".join(out)
    return _dump_scalar(value) + "\n"


def safe_dump(data, stream=None, default_flow_style=False, sort_keys=True,
              **_kwargs):
    """Serialise `data` in block style — enough for text searches over a node."""
    text = _dump_node(data, 0, sort_keys)
    if stream is None:
        return text
    stream.write(text)
    return None
