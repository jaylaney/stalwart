# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Minimal Rust source scanner for the strict-rule tooling (not a parser).

It blanks comments and literal contents while keeping every byte position, then walks brackets to
find functions, their bodies and the top-level statements of a body. Positions are reported as
1-based (line, column) pairs, columns counted in characters, which is how llvm-cov's export reports
region positions for these sources.
"""
import re

IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
ITEMS = {"fn", "use", "struct", "enum", "impl", "const", "static", "mod", "trait", "type", "extern", "pub"}
BLOCKLIKE = {"if", "match", "for", "while", "loop", "unsafe"}


def blank_comments_and_literals(src):
    """Return (nocomment, clean), both as long as src with newlines kept: comments blanked in both,
    literal contents also blanked in clean."""
    n = len(src)
    a = list(src)
    b = list(src)
    i = 0

    def blank(lst, s, e):
        for k in range(s, e):
            if lst[k] != "\n":
                lst[k] = " "

    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(a, i, j)
            blank(b, i, j)
            i = j
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(a, i, j)
            blank(b, i, j)
            i = j
        elif c in "rb" and re.match(r'(br|rb|r)(#*)"', src[i:i + 12]) and not (i > 0 and (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'(?:br|rb|r)(#*)"', src[i:i + 12])
            start = i + m.end()
            end_tok = '"' + m.group(1)
            j = src.find(end_tok, start)
            j = n if j < 0 else j + len(end_tok)
            blank(b, start, j - len(end_tok))
            i = j
        elif c == '"' or (c == "b" and i + 1 < n and src[i + 1] == '"' and not (i > 0 and (src[i - 1].isalnum() or src[i - 1] == "_"))):
            j = i + (2 if c == "b" else 1)
            s = j
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(b, s, j)
            i = j + 1
        elif c == "'":
            if i + 1 < n and src[i + 1] == "\\":
                j = src.find("'", i + 3)
                j = n if j < 0 else j
                blank(b, i + 1, j)
                i = j + 1
            elif i + 2 < n and src[i + 2] == "'" and src[i + 1] != "'":
                blank(b, i + 1, i + 2)
                i += 3
            else:
                i += 1
        else:
            i += 1
    return "".join(a), "".join(b)


def match_bracket(clean, pos):
    """pos at an opening ( [ {; return the index of the matching closer, or -1."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = []
    for i in range(pos, len(clean)):
        c = clean[i]
        if c in pairs:
            stack.append(pairs[c])
        elif c in ")]}":
            if not stack or stack[-1] != c:
                return -1
            stack.pop()
            if not stack:
                return i
    return -1


def line_col(text, off):
    """1-based (line, column) of a character offset."""
    return text.count("\n", 0, off) + 1, off - (text.rfind("\n", 0, off) + 1) + 1


def functions(nocomment, clean):
    """Every `fn` with a body: name, (line, column) of the `fn` keyword, async flag, body offsets
    (opening and closing brace) and the enclosing containers (inline `mod`, `impl`/`trait` header,
    enclosing `fn`)."""
    out, frames, i, n = [], [], 0, len(clean)
    ws = re.compile(r"\s+")
    while i < n:
        c = clean[i]
        if c == "{":
            frames.append(("other", None, i))
            i += 1
            continue
        if c == "}":
            if frames:
                frames.pop()
            i += 1
            continue
        m = IDENT.match(clean, i)
        if not m or (i > 0 and (clean[i - 1].isalnum() or clean[i - 1] == "_")):
            i += 1
            continue
        word, end = m.group(0), m.end()
        if word == "mod":
            m2 = re.compile(r"\s+([A-Za-z_][A-Za-z0-9_]*)\s*([;{])").match(clean, end)
            if m2 and m2.group(2) == "{":
                frames.append(("mod", m2.group(1), m2.end() - 1))
                i = m2.end()
                continue
        if word in ("impl", "trait"):
            j, depth = end, 0
            while j < n:
                ch = clean[j]
                if ch in "([<":
                    depth += 1
                elif ch in ")]>" and depth:
                    depth -= 1
                elif ch == ";" and depth == 0:
                    break
                elif ch == "{" and depth == 0:
                    frames.append((word, ws.sub(" ", nocomment[i:j]).strip(), j))
                    i = j + 1
                    break
                j += 1
            else:
                i = end
            if j < n and clean[j] == "{":
                continue
            i = end
            continue
        if word == "fn":
            m2 = re.compile(r"\s+([A-Za-z_][A-Za-z0-9_]*)").match(clean, end)
            if m2:
                j, depth, body = m2.end(), 0, None
                while j < n:
                    ch = clean[j]
                    if ch in "([":
                        depth += 1
                    elif ch in ")]":
                        depth -= 1
                    elif depth == 0 and ch == ";":
                        break
                    elif depth == 0 and ch == "{":
                        body = j
                        break
                    j += 1
                if body is not None:
                    close = match_bracket(clean, body)
                    prefix = clean[max(0, i - 40):i]
                    out.append({
                        "name": m2.group(1),
                        "fn_pos": line_col(clean, i),
                        "async": bool(re.search(r"\basync\s+(?:unsafe\s+)?$", prefix)),
                        "open": body,
                        "close": close,
                        "containers": [(f[0], f[1]) for f in frames if f[0] in ("mod", "impl", "trait", "fn")],
                    })
                    frames.append(("fn", m2.group(1), body))
                    i = body + 1
                    continue
                i = j + 1
                continue
        i = end
    return out


def _ws(clean, i, end):
    while i < end and clean[i].isspace():
        i += 1
    return i


def _block_open(clean, i, end):
    """First `{` at paren and bracket depth 0 from i (the block of a loop or condition header)."""
    depth = 0
    while i < end:
        ch = clean[i]
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        elif ch == "{" and depth == 0:
            return i
        i += 1
    return -1


def statements(clean, open_, close):
    """Top-level statements of the block whose braces are at open_ and close.

    A statement ends at a `;` at depth 0. A block-like statement (`if`, `match`, `for`, `while`,
    `loop`, a labelled loop, `unsafe { }`, a bare block, a brace macro `name! { }`, or an item) ends
    at its closing brace unless the next token is `else`, `.` or `?`. What remains before `close`
    with no `;` is the expression tail. Each statement has an anchor, the token whose region runs on
    every arrival at the statement: its first token, except for a `for` loop (the first token of the
    iterator expression after `in`) and for `let`, `if let` and `while let` (the first token of the
    scrutinee after `=`), whose patterns' regions count iterations or matches instead.
    """
    out, i = [], open_ + 1
    while True:
        i = _ws(clean, i, close)
        if i >= close:
            break
        while clean.startswith("#[", i):
            e = match_bracket(clean, i + 1)
            i = _ws(clean, e + 1, close)
        start = j = i
        if clean[j] == "'":
            m = re.compile(r"'[A-Za-z_][A-Za-z0-9_]*\s*:\s*").match(clean, j)
            if m:
                j = m.end()
        m = IDENT.match(clean, j)
        word = m.group(0) if m else ""
        if word == "async" and re.compile(r"async\s+(move\s+)?fn\b").match(clean, j):
            word = "fn"
        is_item = word in ITEMS or (word == "macro_rules" and clean[m.end():m.end() + 1] == "!")
        macro = bool(m) and re.compile(r"\s*!").match(clean, m.end()) is not None and word != "macro_rules"
        blocklike = word in BLOCKLIKE or clean[j] == "{" or is_item
        if macro:
            k = re.compile(r"\s*!\s*").match(clean, m.end()).end()
            blocklike = blocklike or (k < close and clean[k] == "{")
        depth, end, k = 0, None, start
        while k < close:
            ch = clean[k]
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
                if depth == 0 and ch == "}" and blocklike:
                    nx = _ws(clean, k + 1, close)
                    if not (clean.startswith("else", nx) and not clean[nx + 4:nx + 5].isalnum()) and clean[nx:nx + 1] not in (".", "?"):
                        end = nx + 1 if clean[nx:nx + 1] == ";" else k + 1
                        break
            elif ch == ";" and depth == 0:
                end = k + 1
                break
            k += 1
        tail = end is None
        if tail:
            end = close
        if is_item:
            kind = "item"
        elif word == "let":
            kind = "let"
        elif word in ("for", "while", "loop") or clean[start] == "'":
            kind = "loop"
        elif word in ("if", "match"):
            kind = word
        elif macro:
            kind = "macro"
        elif clean[start] == "{" or word == "unsafe":
            kind = "block"
        else:
            kind = "expression"
        anchor = start
        loop_block = None
        if word == "let" or (word in ("if", "while") and re.compile(r"(if|while)\s+let\b").match(clean, j)):
            # `let PAT = EXPR`, `if let PAT = EXPR`, `while let PAT = EXPR`: the pattern's region counts
            # matches, the scrutinee's counts arrivals, so the anchor is the first token after the `=`
            d2, q = 0, j
            while q < end:
                ch = clean[q]
                if ch in "([{":
                    d2 += 1
                elif ch in ")]}":
                    d2 -= 1
                elif d2 == 0 and ch == "=" and clean[q + 1:q + 2] not in ("=", ">") and clean[q - 1:q] not in ("=", "!", "<", ">"):
                    anchor = _ws(clean, q + 1, end)
                    break
                q += 1
        if kind == "loop":
            if word == "for":
                d2, q = 0, j + 3
                while q < end:
                    ch = clean[q]
                    if ch in "([{":
                        d2 += 1
                    elif ch in ")]}":
                        d2 -= 1
                    elif d2 == 0 and clean.startswith("in", q) and clean[q - 1].isspace() and clean[q + 2:q + 3].isspace():
                        anchor = _ws(clean, q + 2, end)
                        break
                    q += 1
                bo = _block_open(clean, anchor, end)
            else:
                bo = _block_open(clean, max(anchor, j + len(word)), end)
            if bo >= 0:
                loop_block = (bo, match_bracket(clean, bo))
        out.append({"start": start, "end": end, "kind": kind, "tail": tail, "anchor": anchor, "loop_block": loop_block})
        i = end
    return out


def last_statement(clean, open_, close):
    """The body's last top-level statement that is not an item, or None for an empty body."""
    st = [s for s in statements(clean, open_, close) if s["kind"] != "item"]
    return st[-1] if st else None


def first_statement(clean, open_, close):
    st = [s for s in statements(clean, open_, close) if s["kind"] != "item"]
    return st[0] if st else None
