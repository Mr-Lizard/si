/*
  Some constants used throughout the diagram
  Much of this may get migrated into a DiagramConfiguration object as default values
  so that most of it can be overridden if necessary... but we'll wait until we see the need
*/

// how far a user must move the mouse after initial click to start "dragging"
export const DRAG_DISTANCE_THRESHOLD = 5;

// if dragging to the edge of the screen, within this area (pixels) will trigger scrolling in that direction
export const DRAG_EDGE_TRIGGER_SCROLL_WIDTH = 15;

// spacing between sockets
export const SOCKET_GAP = 22;
// spacing above/below sockets within the node
export const SOCKET_MARGIN_TOP = 10; // less because there is also a subtitle with some padding
export const SOCKET_MARGIN_BOTTOM = 20; //
// width/height of sockets
export const SOCKET_SIZE = 15;

// corner radius used on nodes (maybe other things later?)
export const CORNER_RADIUS = 3;

// default node color if nothing passed in
// TODO: this is a random purple color... check with mark
export const DEFAULT_NODE_COLOR = "#8B39CB";

// font family used for all text elements
export const DIAGRAM_FONT_FAMILY = "Inter";

// color used to show what is selected (currently a nice blue)
// TODO: pull from tailwind?
export const SELECTION_COLOR = "#59B7FF";
