; Inherit all standard Lua highlights from tree-sitter-lua.
; This file adds GLua-specific overrides on top.

; Keywords (anonymous tokens that the grammar exposes as queryable literals)
[
  "and"
  "do"
  "else"
  "elseif"
  "end"
  "for"
  "function"
  "goto"
  "if"
  "in"
  "local"
  "not"
  "or"
  "repeat"
  "return"
  "then"
  "until"
  "while"
] @keyword

; `break_statement` is defined as a single-string rule in tree-sitter-lua, so
; the literal "break" isn't a queryable anonymous node — we must match the
; named node instead.
(break_statement) @keyword

; GLua adds `continue`, which standard Lua doesn't have. The grammar parses it
; as a plain identifier, so highlight by text match.
((identifier) @keyword
 (#eq? @keyword "continue"))

; Functions
(function_declaration
  name: (identifier) @function)

(function_declaration
  name: (dot_index_expression) @function)

(function_declaration
  name: (method_index_expression) @function)

(function_call
  name: (identifier) @function.call)

(function_call
  name: (dot_index_expression
    field: (identifier) @function.call))

(function_call
  name: (method_index_expression
    method: (identifier) @function.call))

; Parameters
(parameters
  name: (identifier) @variable.parameter)

; Types / class names (common GMod globals)
((identifier) @type
 (#match? @type "^[A-Z][A-Z0-9_]+$"))

; Strings
(string
  start: _ @punctuation.special
  content: (string_content) @string
  end: _ @punctuation.special)

; String escape sequences
(escape_sequence) @string.escape

; Numbers
(number) @number

; Booleans / nil
(true) @boolean
(false) @boolean
(nil) @constant.builtin

; Vararg (also a single-string rule — match named node)
(vararg_expression) @variable.special

; Operators — standard Lua
; Note: "//" is intentionally absent — in GLua it is a line comment.
[
  "+"  "-"  "*"  "/"  "%"  "^"  "#"
  "&"  "~"  "|"  "<<"  ">>"
  "=="  "~="  "<"  "<="  ">"  ">="
  "="
  "("  ")"  "{"  "}"  "["  "]"
  "::"
  ";"  ":"  ","  "."  ".."
] @operator

; GLua C-style operators — these are just tokenized as identifiers or
; punctuation by the Lua grammar when nonstandardSymbol is configured,
; so we highlight them as operators by matching the raw token text.
; glua-ls handles the semantic understanding; we just need them visible.
((identifier) @operator
 (#any-of? @operator "&&" "||" "!=" "!"))

; Comments
(comment) @comment
(hash_bang_line) @comment

; Variables
(identifier) @variable

; Self
((identifier) @variable.special
 (#eq? @variable.special "self"))

; Fields
(dot_index_expression
  field: (identifier) @property)

(bracket_index_expression
  field: (identifier) @property)

; Labels
(label_statement
  (identifier) @label)

(goto_statement
  (identifier) @label)
