export function isTimelineVisible(
  bounds: { top: number; bottom: number; height: number },
  viewportHeight: number,
) {
  const visible = Math.max(0, Math.min(bounds.bottom, viewportHeight) - Math.max(bounds.top, 0));
  return bounds.height > 0 && visible >= Math.min(bounds.height * 0.8, viewportHeight * 0.6);
}

export function wheelTarget(current: number, target: number, delta: number, maximum: number) {
  // Reverse from the visible position immediately, without waiting for an old target.
  const origin = (target - current) * delta < 0 ? current : target;
  return Math.max(0, Math.min(maximum, origin + delta));
}

export function slidePosition(current: number, target: number, elapsed: number) {
  const next = current + (target - current) * (1 - Math.exp(-Math.max(0, elapsed) / 40));
  return Math.abs(target - next) < 0.5 ? target : next;
}
