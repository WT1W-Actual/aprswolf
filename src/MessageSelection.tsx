import { type Selection } from "./lib/useSelection";

/** Class for a message row: laid out for its checkbox while selecting, with room for ↺ when hidden. */
export function rowClass(selection: Selection, hidden?: boolean): string {
  return `msg-row${selection.active ? " selecting" : ""}${hidden ? " has-restore" : ""}`;
}

// The checkbox at a row's left edge while selecting.
export function SelectBox({ selection, uuid }: { selection: Selection; uuid?: string }) {
  if (!selection.active || !uuid) return null;
  return (
    <input
      type="checkbox"
      className="msg-select-box"
      checked={selection.isSelected(uuid)}
      onChange={() => selection.toggle(uuid)}
      aria-label="Select message"
    />
  );
}

// Beside a pane title: Clear all normally; the selected count and Done
// while selecting.
export function SelectionHeader({ selection, onClearAll }: { selection: Selection; onClearAll: () => void }) {
  if (selection.active) {
    return (
      <>
        <span className="msg-select-count">{selection.count} selected</span>
        <button type="button" className="msg-clear-all-btn" onClick={selection.exit}>
          Done
        </button>
      </>
    );
  }
  return (
    <button type="button" className="msg-clear-all-btn" onClick={onClearAll} title="Clear the messages this list shows now">
      Clear all
    </button>
  );
}
