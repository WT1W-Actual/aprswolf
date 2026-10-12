// The controls at a message row's right edge, centered on the row: × to
// clear it, plus, for a hidden message found by search, a HIDDEN label that
// always shows and ↺ to put it back in its list.
function MessageRowActions({
  hidden,
  onClear,
  onRestore,
}: {
  hidden?: boolean;
  onClear: () => void;
  onRestore: () => void;
}) {
  return (
    <div className="msg-row-actions">
      {hidden ? (
        <span className="msg-hidden-badge" title="Cleared from the list; kept in the database">hidden</span>
      ) : null}
      {hidden ? (
        <button type="button" className="msg-row-btn msg-restore-btn" aria-label="Unhide this message" title="Unhide: show it in the list again" onClick={onRestore}>
          ↺
        </button>
      ) : null}
      <button type="button" className="msg-row-btn msg-clear-btn" aria-label="Clear this message" title="Clear this message" onClick={onClear}>
        ×
      </button>
    </div>
  );
}

export default MessageRowActions;
