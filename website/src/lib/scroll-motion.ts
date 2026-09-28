export function scrollDuration(distance: number) {
  return Math.min(620, Math.max(360, 280 + Math.abs(distance) * 0.18));
}

export function glidePosition(start: number, target: number, elapsed: number, duration: number) {
  const progress = Math.min(1, Math.max(0, elapsed / Math.max(1, duration)));
  const eased = 1 - (1 - progress) ** 3;
  return progress === 1 ? target : start + (target - start) * eased;
}
