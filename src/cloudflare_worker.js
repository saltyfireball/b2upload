const VERSION = "1.1.2";

export default {
    async fetch(request, env) {
        const url = new URL(request.url);
        const path = url.pathname;

        if (path === "/version") {
            return new Response(VERSION);
        }

        const token = url.searchParams.get("token");
        const expires = url.searchParams.get("expires");

        if (!token || !expires) {
            return new Response("Unauthorized", { status: 401 });
        }

        // Check expiry first (expires is a Unix timestamp in seconds)
        const now = Math.floor(Date.now() / 1000);
        if (now > parseInt(expires, 10)) {
            return new Response("Link expired", { status: 403 });
        }

        // Validate token — signed over path + expires together
        const expectedToken = await generateToken(
            path,
            expires,
            env.TOKEN_SECRET,
        );

        if (token !== expectedToken) {
            return new Response("Unauthorized", { status: 401 });
        }

        // Strip params before proxying to B2
        url.searchParams.delete("token");
        url.searchParams.delete("expires");

        const b2Url = `${env.B2_ORIGIN_URL}${path}`;
        const response = await fetch(b2Url);

        const headers = new Headers(response.headers);
        headers.set("Access-Control-Allow-Origin", "*");

        return new Response(response.body, {
            status: response.status,
            headers,
        });
    },
};

async function generateToken(path, expires, secret) {
    const message = `${path}:${expires}`;
    const encoder = new TextEncoder();
    const key = await crypto.subtle.importKey(
        "raw",
        encoder.encode(secret),
        { name: "HMAC", hash: "SHA-256" },
        false,
        ["sign"],
    );
    const signature = await crypto.subtle.sign(
        "HMAC",
        key,
        encoder.encode(message),
    );
    const base64 = btoa(String.fromCharCode(...new Uint8Array(signature)));
    return base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
