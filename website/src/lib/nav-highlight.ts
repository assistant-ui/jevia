interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Use content-relative, fractional geometry so both vertical and horizontal rails align. */
export function navHighlightBox(item: Box, container: Box) {
  return {
    x: item.left - container.left,
    y: item.top - container.top,
    width: item.width,
    height: item.height,
  };
}

export function navHighlightTarget<Id extends string>(
  active: Id,
  hovered: Id | null,
  focused: Id | null,
): Id {
  return hovered ?? focused ?? active;
}
