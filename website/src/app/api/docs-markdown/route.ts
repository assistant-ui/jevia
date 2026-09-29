import { GET as createGetEndpoint } from "@farm.js/core/api";

import docsMarkdown from "../../docs/page.md?raw";

export const GET = createGetEndpoint(() =>
  new Response(docsMarkdown, {
    headers: {
      "cache-control": "public, max-age=0, must-revalidate",
      "content-disposition": 'inline; filename="docs.md"',
      "content-type": "text/markdown; charset=utf-8",
    },
  }),
);
