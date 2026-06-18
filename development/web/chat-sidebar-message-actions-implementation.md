# Chat Sidebar And Message Actions Implementation

## Summary

This pass implements the frontend-visible fixes from `allthecodes-web/development-docs/human-check.md` and adds backend session message action routes for the new chat controls.

Implemented:

- Sidebar project rows hide path/session count, support collapse/expand, and show project actions on hover.
- Chat grid close removes a slot and shrinks the layout instead of leaving empty white space.
- Chat tiles show session title or `Untitled session`, never a session id prefix.
- Chat output uses a GFM-capable Markdown renderer.
- User and assistant messages expose copy, branch, delete, edit/regenerate, feedback, usage, and rollback entry points.
- Backend session routes support branch, delete, regenerate prepare, edit prepare, feedback, and rollback preview.

## Backend Routes

- `POST /api/sessions/{id}/messages/{message_id}/branch`
- `POST /api/sessions/{id}/messages/{message_id}/feedback`
- `POST /api/sessions/{id}/messages/{message_id}/delete`
- `POST /api/sessions/{id}/messages/{message_id}/regenerate/prepare`
- `POST /api/sessions/{id}/messages/{message_id}/edit/prepare`
- `POST /api/sessions/{id}/messages/{message_id}/rollback/preview`
- `POST /api/sessions/{id}/messages/{message_id}/rollback`

## Rollback Checkpoint Follow-Up

The user selected message-scoped rollback, not simple uncommitted Git rollback. That requires durable file checkpoints around each user turn.

Current behavior:

- Rollback preview validates the session/message and returns `available: false` when no checkpoint exists.
- Rollback execute returns `501 rollback_checkpoint_unavailable`.

Required follow-up:

- Record file state before each user turn and changed paths after the turn.
- Persist checkpoint metadata alongside session history.
- Make preview return the exact files modified after the selected user message.
- Make execute restore selected files to the checkpoint state after user confirmation.

## Verification

- Frontend `npm run typecheck`
- Frontend `npm run build`
- Backend `cargo check -p allthecodes-web`
