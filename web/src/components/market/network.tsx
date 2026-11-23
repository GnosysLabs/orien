/** The network's own look in the community: its avatar and the names of
 * its kinds of news. */

/** The name over each kind of news the network posts. */
export const EYEBROWS: Record<string, string> = {
  record: "New record",
  welcome: "Welcome",
  milestone: "Milestone",
  streak: "On a streak",
  top_day: "Top miner of the day",
  swap: "Swap",
  supply: "ORN mined",
  validator: "New validator",
};

/** The network, as a poster: the Orien sun on living pearl. */
export function NetworkAvatar({ size = 36 }: { size?: number }) {
  return (
    <span
      className="pearl-fill flex shrink-0 items-center justify-center rounded-full"
      style={{ width: size, height: size }}
      aria-hidden
    >
      <span
        className="orien-mark bg-bg"
        style={{ width: size * 0.56, height: size * 0.56 }}
      />
    </span>
  );
}
