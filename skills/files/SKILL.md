# Files

Work with files in the user's working folder and any extra folders the user allowed.

- Relative paths resolve inside the working folder (shown in the system prompt).
- Read before you edit: use `fs_read`, then `fs_edit` with an exact `old` snippet.
- `fs_write` refuses to overwrite unless `overwrite=true`; ask the user before overwriting.
- Deleting, moving and writing need the user's approval by default. Explain what you are about to change.
- Paths with `..` and anything inside Alice's own data folder are refused.
