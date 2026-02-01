import http from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join } from "node:path";

const root = new URL("../src/", import.meta.url).pathname;
const port = 1420;

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
    const urlPath = decodeURIComponent(new URL(req.url ?? "/", "http://localhost").pathname);
    const safePath = urlPath === "/" ? "/index.html" : urlPath;
    const filePath = join(root, safePath);

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

server.listen(port, () => {
  console.log(`[dev-server] http://localhost:${port}`);
});
