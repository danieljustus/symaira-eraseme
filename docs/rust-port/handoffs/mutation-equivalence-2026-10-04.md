# Mutation classification at source 30b38e2

The actual complete HTTP campaign in run `37174075472` executed 102 mutants:
75 caught, 16 unviable and 11 missed. The original artifact is retained;
this result fails acceptance. Two missed mutants expose real test gaps:
the worker-panic error code at HTTP line 305 and the body-collector error
code at line 311. Tests now exercise both through the actual TCP/Hyper
request handler. They remain in the mutation catalog.

Four HTTP mutants affect code compiled out on the Linux campaign target.
The non-Linux bind diagnostic at line 190 has two string substitutions;
the non-Unix token writer and directory creator at lines 239 and 256 each
have one `Ok(())` substitution. Their exact anchors are excluded only in
the Linux HTTP job, with its target and exclusions retained in the receipt.
The global configuration includes no exclusion of these platform branches.
Native bind-diagnostic and token-file tests retain independent expectations
on every supported OS. This campaign establishes Linux mutation coverage;
it does not establish a native mutation run on Windows or macOS.

The following exact mutants preserve their functions' observable result for
every admitted input. Each global exclusion retains a line, column, operator
and function anchor; neighboring mutations stay selected.

| Anchor | Reason for equivalence |
| --- | --- |
| HTTP 290:69, `MAX_BODY_BYTES + 1` to `MAX_BODY_BYTES * 1` | Bodies at or below the limit reach identical protocol dispatch. A body one byte above it takes either the successful collection/oversized branch or the collection-error branch; both produce the identical HTTP 200 / JSON-RPC -32700 response before any handler runs. Larger bodies fail collection in both variants. Declared-length rejection is unchanged. |
| HTTP 389:17 `<` to `<=`, 389:23 `-` to `+` or `/` | `one_json_value` has validated the whole array with the same scanner. An empty array returns earlier. After each item, the loop breaks at the closing bracket; a comma can only be followed by another valid item. Thus no changed bound admits or omits an additional iteration. |
| HTTP 391:35, removing the negative parse-error sign | The same scanner already accepted every item during whole-array validation, and the immutable body is unchanged. This fallback is unreachable after that successful precondition. |
| PII 364:18 `<` to `<=` | An extra iteration at the end sees valid empty UTF-8, searches an escaped nonempty literal, finds no matches and breaks. No result or limit changes. |
| PII 378:22 `>` to `>=` | With zero valid prefix bytes, the extra search likewise sees empty UTF-8 and a nonempty escaped literal; it cannot append a match or fail. Offset advancement is unchanged. |
| PII 437:22, `at + 1` to `at - 1` or `at * 1` | The unchanged first guard returns for `at == 0`. For a trailing at-sign, the unchanged subsequent split produces an empty domain label and rejects it. Otherwise both checks admit the same value, and subsequent multiple-at-sign and label validation remains unchanged. |
| PII 536:25 `<` to `==` or `<=` | The unchanged passport regular expression's identifier capture has length 6 through 9. All three comparisons with 2 are false for every admitted capture. |
| PII 557:23 `>` to `==` or `>=` | Input length is at most 16 MiB and the collector admits at most 100000 matches. Every built-in or fixed profile replacement grows its source span by less than 64 bytes: literal placeholders are shorter, masks append at most a small fixed prefix/suffix, and email/passport output keeps its source width. Thus every intermediate output estimate is below 16 MiB + 6400000 bytes, strictly below the conservative 32 MiB fallback. None of these three predicates can become true. Public review accepts arbitrary replacement bytes and retains its separate, reachable limits and tests. |
| Review 99:18 `index + 1` to `index * 1` | The condition only breaks on the last item and has no subsequent statement within the loop. The unchanged finite iterator terminates naturally there even when this redundant condition is false. |

Actual sanitization shards 1/4 and 2/4 contain twelve missed mutants.
The other eight expose real gaps and stay selected: inclusive input length,
complete invalid-SSN filtering, unequal prefix/suffix mask widths, and exact
short-email/phone boundaries. Neither unfinished shards nor partial original
campaigns are accepted. Every subsequent run must retain a complete catalog,
execute all disjoint selected mutants, pass the baseline and report zero
missed mutants with the existing 30-second and 45-minute limits.

The complete sanitization shard 3/4 additionally found 23 missed mutants.
Five are the exact equivalents above; the other eighteen remain selected.
New tests preserve review error diagnostics, each action's counts, inclusive
input/match/replacement/projected-output limits, exact 32 MiB output and
overflow caused by keeping a shrinking match. A callback must not run again
after an intermediate limit is exceeded; the final tail is checked separately.
These tests use the original public limits and real allocated byte buffers.

The complete consent campaign found five missed mutants. Its three Windows
close mutants affect cfg-disabled code on Linux and are excluded only in
the Linux consent command. After the local repair, their exact anchors are
628:5 `close_windows_file`, 640:5 `checked_windows_close`, and 644:39
the latter's `!=` comparison. The existing native Windows tests keep the
actual invalid-handle error and old-file rollback control; no global
exclusion masks them. The other two remain selected: directory-error
translation and checked file-close failure. Operation-local boundaries
retain the native create/close operations, propagate the original errors
and permit deterministic fault controls without global hooks. The Unix
close control closes a valid owner once, then obtains actual kernel EBADF
from descriptor -1 without constructing a stale owner. It requires the old
file and unrelated entries to survive, and the owned temporary to be removed.
