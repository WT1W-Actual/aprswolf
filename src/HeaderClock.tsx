import { useEffect, useState } from "react";

const utcFormat = new Intl.DateTimeFormat("en-GB", {
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
  timeZone: "UTC",
});

const localFormat = new Intl.DateTimeFormat("en-GB", {
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
});

/** Short name of the local time zone at `when`, such as PDT or GMT+2. */
function zoneName(when: Date): string {
  const part = new Intl.DateTimeFormat("en-US", { timeZoneName: "short" })
    .formatToParts(when)
    .find((p) => p.type === "timeZoneName");
  return part?.value ?? "local";
}

/** "15:08Z 08:08 PDT": UTC and local time, 24-hour hh:mm. */
export function formatHeaderClock(when: Date): string {
  return `${utcFormat.format(when)}Z ${localFormat.format(when)} ${zoneName(when)}`;
}

// UTC and local time for the header row, redrawn as each minute turns.
function HeaderClock() {
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    let timer: number;
    const tick = () => {
      const current = new Date();
      setNow(current);
      timer = window.setTimeout(tick, 60_000 - (current.getTime() % 60_000) + 50);
    };
    timer = window.setTimeout(tick, 60_000 - (Date.now() % 60_000) + 50);
    return () => window.clearTimeout(timer);
  }, []);

  return (
    <span className="header-clock" title="UTC and local time">
      {formatHeaderClock(now)}
    </span>
  );
}

export default HeaderClock;
