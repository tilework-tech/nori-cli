# Remove HTTP Backend

## Goal

Surgically remove the HTTP backend from the nori-cli codebase. The HTTP backend is causing confusion for interns and coding agents. Only the ACP backend matters.

## Approach

Since the HTTP backend is deeply integrated, we cannot remove it all at once. We will remove it incrementally, one component at a time, ensuring the ACP backend continues to work correctly after each removal.

## Constraints

- Each commit should remove a single, well-defined component of the HTTP backend
- The ACP backend must continue to work correctly after each removal
- All tests must pass after each change
- Documentation must be updated to remove HTTP backend references
