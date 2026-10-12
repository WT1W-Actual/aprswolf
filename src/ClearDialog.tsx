import { useEffect, useState } from "react";

export type ClearRequest = {
  /** What is being cleared, for the dialog title: "this message", "all 42 sent messages". */
  what: string;
  /** Every message named is already hidden, so the only thing left is deleting it. */
  alreadyHidden?: boolean;
  run: (deleteFromDatabase: boolean) => Promise<void> | void;
};

// Confirms clearing one message or a whole list. Unchecked, the messages
// leave the list and stay in the database, where search still finds them;
// checked, they are deleted from the database.
function ClearDialog({ request, onClose }: { request: ClearRequest | null; onClose: () => void }) {
  const [deleteFromDatabase, setDeleteFromDatabase] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setDeleteFromDatabase(Boolean(request?.alreadyHidden));
    setBusy(false);
  }, [request]);

  useEffect(() => {
    if (!request) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [request, onClose]);

  if (!request) return null;

  async function confirm() {
    if (!request) return;
    setBusy(true);
    try {
      await request.run(deleteFromDatabase);
    } finally {
      onClose();
    }
  }

  return (
    <div className="reply-overlay" role="dialog" aria-modal="true" aria-label="Clear messages">
      <div className="reply-dialog clear-dialog">
        <h2>Clear {request.what}?</h2>
        <label className="clear-dialog-option">
          <input
            type="checkbox"
            checked={deleteFromDatabase}
            disabled={request.alreadyHidden}
            onChange={(e) => setDeleteFromDatabase(e.currentTarget.checked)}
          />
          Delete from database
        </label>
        <p className="clear-dialog-note">
          {deleteFromDatabase
            ? "Deleted for good. Search will not find it again."
            : "Hidden from the list but kept in the database. Search still finds it, marked as hidden."}
        </p>
        <div className="reply-actions">
          <button type="button" className={deleteFromDatabase ? "danger" : ""} onClick={() => void confirm()} disabled={busy} autoFocus>
            {deleteFromDatabase ? "Delete" : "Clear"}
          </button>
          <button type="button" className="secondary" onClick={onClose}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

export default ClearDialog;
