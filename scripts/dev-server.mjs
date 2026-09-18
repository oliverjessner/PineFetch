import http from "node:http";
import { readFile, realpath, stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = await realpath(fileURLToPath(new URL("../src/", import.meta.url)));
const port = 1420;
const host = "127.0.0.1";

const mime = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "application/javascript",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".svg": "image/svg+xml",
};

const server = http.createServer(async (req, res) => {
  try {
    const urlPath = decodeURIComponent(new URL(req.url ?? "/", `http://${host}`).pathname);
    const safePath = urlPath === "/" ? "/index.html" : urlPath;
    const candidate = resolve(root, `.${safePath}`);
    if (!candidate.startsWith(`${root}${sep}`)) {
      res.writeHead(404);
      res.end("Not Found");
      return;
    }
    const filePath = await realpath(candidate);
    if (!filePath.startsWith(`${root}${sep}`)) {
      res.writeHead(404);
      res.end("Not Found");
      return;
    }

    const fileStat = await stat(filePath);
    if (!fileStat.isFile()) {
      res.writeHead(404);
      res.end("Not Found");
      return;
    }

    const data = await readFile(filePath);
    const ext = extname(filePath);
    res.writeHead(200, { "Content-Type": mime[ext] || "application/octet-stream" });
    res.end(data);
  } catch {
    res.writeHead(404);
    res.end("Not Found");
  }
});

server.listen(port, host, () => {
  console.log(`[dev-server] http://${host}:${port}`);
});
