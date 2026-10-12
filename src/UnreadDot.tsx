// Red count bubble for unread To Me messages, shown on the To Me filter chip.
function UnreadDot({ count }: { count: number }) {
  if (count <= 0) return null;
  return (
    <span className="unread-dot" title={`${count} unread`}>
      {count > 99 ? "99+" : count}
    </span>
  );
}

export default UnreadDot;
