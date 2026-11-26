# 0048: Move chosen leftovers to the Trash, after checking them again

- **Date:** 2026-11-26
- **Branch / PR:** `feat/trash`, #38
- **Plan chunks:** 4.4, done
- **Commits:** 059494b, 5ab33c9

## What changed
- **New core module `trash.rs`** (decision D43).
  - `trash(host, path, now)` moves an item into the freedesktop home trash: `$XDG_DATA_HOME/Trash/files/<name>`, plus
    `info/<name>.trashinfo` holding the original path (percent-encoded) and the deletion date.
  - The info file is created first, and only when no other info file has that name, so writing it claims the name.
  - A name taken in `info/` or `files/` gets `.2`, `.3`… up to 1000 tries. Nothing in the Trash is overwritten.
  - The item moves by rename. An item on another file system is refused (`TrashError::OtherFileSystem`), never copied
    and deleted, and the claimed info file is removed again.
  - Errors: `Missing`, `OtherFileSystem`, `Changed { reason }`, `Io`.
- **Leftovers** (decisions D37, D38).
  - **Identity.** `Leftover::identity` (`FileIdentity`: device, inode, owner) is recorded when a folder is found,
    without following symlinks. It is `None` on systems without Unix metadata.
  - **Checks before trashing.** `Housekeeper::trash_leftover(&Leftover)` moves a folder to the Trash only when all of
    these still hold:
    - there is no symbolic link anywhere on its path (`Host::canonical` gives back the same path);
    - it is still directly inside a folder leftovers are looked for in;
    - it is still a folder;
    - its identity is unchanged.

    Otherwise the folder stays where it is and the error says why.
  - **Preselection.** `Leftover::preselected()` is true for a cache with no `also_used_by` and no `unseen_copies`.
  - The folders leftovers are looked for in are listed by one function, which finding and checking share.
- **`remove --dry-run`** marks each folder:
  - "goes to the Trash with it" when it is preselected;
  - "yours: kept unless you choose it" for settings, data and state;
  - "kept unless you choose it" for a cache something else may use.
- **`Host` gains `create_new`, `rename`, `remove_file`, `create_dir_all` and `symlink_metadata`.** `text::utc_timestamp`
  now formats dates for both the Trash and `housekeeper history`, replacing the CLI's own copy.
- **Docs:**
  - new decision D43;
  - D40 says that trashing system paths joins the helper in 4.5, not 4.4: leftovers are always the user's;
  - D1–D43 in AGENTS.md, docs/README.md and the guide;
  - README's leftovers bullet, CHANGELOG (Changed), and the plan (4.4 ✅).

## Why
Plan chunk 4.4 and decisions D7, D37, D38.

- **Why the Trash.** Leftovers hold user data, so they must be recoverable: the Trash, not deletion. Moving them must
  never overwrite anything already in the Trash.
- **Why check again.** A folder can change between the review and the move; the checks stop Housekeeper acting on
  something nobody reviewed.
- **Why not `gio trash`.** Housekeeper writes the Trash itself, because `gio` is missing on systems without GLib's
  tools, and core must stay testable through `Host`.

## How it was verified
- **New tests:**
  - core `trash.rs`:
    - a folder named `My Tool` moves to `Trash/files/My Tool`, with its contents, and with the info file's exact text
      (`Path=/home/user/.cache/My%20Tool`, `DeletionDate=2026-11-09T06:47:23`);
    - with `tool` taken in `files/` and `tool.2` held by a stray info file, it becomes `tool.3`, and the old `tool` is
      untouched;
    - a missing item is an error that leaves no info file.
  - core `leftovers.rs`:
    - a found cache is preselected, has an identity, goes to the Trash, and a second attempt reports it missing;
    - a leftover swapped for a symbolic link to `~/Documents` is refused, and the documents stay;
    - a leftover replaced by a new folder of the same name is refused as not the same folder.
  - core `text.rs`: timestamps for the epoch, a 2026 date, a leap day and a negative time (moved from the CLI).
  - CLI: the dry run shows all three marks.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 56 + 2 + 151)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - the Windows check of core and platform
  - `cargo deny` was not run (not installed); no dependency was added.
- **The real Trash on this Ubuntu 26.04 machine**, with a throwaway program calling `trash` (deleted afterwards):
  - `~/.cache/hk trash probe` moved to `~/.local/share/Trash/files/hk trash probe`.
  - `gio list trash://` showed it with `trash::orig-path=/home/…/.cache/hk trash probe`.
  - `gio trash --restore` put it back with its file, and I then removed it.
- **`remove --dry-run` here:**
  - the uv binary's `~/.cache/uv` shows "goes to the Trash with it", and `~/.config/uv` "yours: kept unless you choose
    it";
  - for `dpkg:pipx`, `~/.cache/pipx` goes, and `~/.local/state/pipx` is kept.

## Known limits and follow-ups
- **Deletion dates are in UTC.** The Trash shows them hours off in other time zones; the specification asks for local
  time, which needs a time-zone database.
- **Leftovers on another file system are refused** (a `~/.cache` on its own mount, for example). The specification's
  per-mount `$topdir/.Trash-$uid` is not used yet.
- **A symlinked leftover folder is refused.** When `~/.config` itself is a symbolic link (dotfile managers), folders in
  it are found but trashing them is refused, because the path holds a link.
- **A short window remains.** It lies between the checks and the rename; inside it, a swapped final component would move
  the link, not its target, into the Trash, which can be put back.
- **Nothing trashes yet.** `housekeeper remove` will trash the chosen leftovers in 4.5.
