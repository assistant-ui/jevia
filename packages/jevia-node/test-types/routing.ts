import { JeviaClient, type RouteOptions } from "../src/index.js";

const client = new JeviaClient();
const options: RouteOptions = {
  useHistory: false,
  noCache: true,
  signal: new AbortController().signal,
};
void client.route("task", options);
void client.route("task", { useHistory: true });
void client.route("task");
// @ts-expect-error history preference must be a boolean
void client.route("task", { useHistory: "false" });
// @ts-expect-error null is not a history preference
void client.route("task", { useHistory: null });
