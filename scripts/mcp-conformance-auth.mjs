// Only supply the disposable, owned server's bearer header. Keep official
// SDK requests, responses and conformance assertions intact.
const endpoint = new URL(process.env.MCP_PROBE_ENDPOINT);
const token = process.env.MCP_PROBE_BEARER;
if (endpoint.hostname !== '127.0.0.1' || endpoint.protocol !== 'http:' ||
    !/^[A-Za-z0-9_-]{43}$/.test(token)) {
  throw new Error('Expected an owned loopback endpoint and disposable token');
}
const original = globalThis.fetch;
globalThis.fetch = (input, init = {}) => {
  const url = new URL(input instanceof Request ? input.url : input);
  if (url.origin !== endpoint.origin || url.pathname !== endpoint.pathname) {
    throw new Error('Conformance probe attempted an unowned endpoint');
  }
  const headers = new Headers(input instanceof Request ? input.headers : undefined);
  for (const [name, value] of new Headers(init.headers)) headers.set(name, value);
  headers.set('Authorization', `Bearer ${token}`);
  return original(input, { ...init, headers });
};
