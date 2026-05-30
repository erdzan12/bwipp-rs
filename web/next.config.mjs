/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  async headers() {
    return [
      {
        // The prebuilt WASM bundle is served from public/wasm/ at a stable
        // filename (/wasm/bwipp_wasm.wasm). Force revalidation so a freshly
        // deployed bundle is never shadowed by a stale browser/CDN cache.
        // The file carries an ETag, so unchanged bytes still return a cheap
        // 304 — only a genuinely new bundle is re-downloaded.
        source: "/wasm/:file*",
        headers: [
          { key: "Cache-Control", value: "public, max-age=0, must-revalidate" },
        ],
      },
    ];
  },
};

export default nextConfig;
