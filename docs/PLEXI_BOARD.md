# Stint board for Plexi

An optional priority board that runs inside Plexi. It is a separate package,
not part of the `stint` crate and not bundled with Plexi. It shows tasks in
priority columns, searches, shows detail, and changes priority. It runs only
the `stint` CLI, through operations Plexi asks you to approve. It never reads
`.stint` files itself.

The board version always matches the CLI version. Install both from the same
release. If they differ, the board shows an "Incompatible stint version"
screen with this guidance.

## Requirements

- Plexi <plexi_min> or newer with the `cli_operations` experiment enabled:
  `plexi experiments enable cli_operations`, then `plexi host restart`.
- The `stint` CLI at the matching version, installed where Plexi looks for
  programs (`~/.cargo/bin` is searched).

## Install

1. Install the matching CLI (the version printed here, <version>, is the one this board is built for):

   ```sh
   cargo install stint --version <version> --locked --force
   stint --version
   ```

2. Get the board package for the same release, `plexi-stint-<version>.plexipkg`
   (a release asset), or build it from a checkout of this repository:

   ```sh
   just plexi-package          # writes target/plexi/plexi-stint-<version>.plexipkg
   ```

3. Install it and review the trust sheet:

   ```sh
   plexi app install target/plexi/plexi-stint-<version>.plexipkg
   ```

4. Open a Plexi pane in a workspace that has a `.stint/` directory and run
   `plexi app open plexi-stint` (or open "Stint Board" from the palette).

## Approval

The first time the board needs something, Plexi asks you in the Attention
queue. Reading (version, list, show) and changing priority are approved
separately. Deny the change and the board stays a read-only browser. The
approval states that `stint` runs with your full account authority: Plexi
pins which program runs but does not confine it.

## Keys

`h j k l` move, `0`-`4` set priority, `/` search, `r` refresh or retry a
conflicted change, `e` ask for edit permission again. Clicking a row selects
it; the priority buttons change it. A change applies only if the task still
has the priority the board showed. If someone else changed it first, the board
keeps their value and offers an explicit retry.

## Upgrade

Install the new CLI first, then the new package. Changed operations or a new
`stint` binary ask for approval again.
