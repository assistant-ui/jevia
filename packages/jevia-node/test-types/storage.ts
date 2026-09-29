import { JeviaClient, type StorageTarget } from "../src/index.js";

const client = new JeviaClient();
const sqlite: StorageTarget = { backend: "sqlite", path: ".jevia/history.db" };
void client.setupStorage(sqlite);
void client.setupStorage(sqlite, { apply: true, confirmStopped: true, importJsonl: true });
void client.setupStorage({ backend: "postgres", project: "app", urlEnv: "APP_DB" });
void client.checkStorage({ deep: true, signal: new AbortController().signal });
// @ts-expect-error apply requires writer confirmation
void client.setupStorage(sqlite, { apply: true });
// @ts-expect-error confirmation is only allowed with apply
void client.setupStorage(sqlite, { confirmStopped: true });
// @ts-expect-error a PostgreSQL project scope is required
void client.setupStorage({ backend: "postgres" });
// @ts-expect-error credentials belong in environment, not arguments
void client.setupStorage({ backend: "postgres", project: "app", url: "postgres://secret" });
