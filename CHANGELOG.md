# Changelog

## Unreleased

### Added

- Logins for web-service connections (WMS, WMTS, WFS, WCS, OGC API, ArcGIS REST, CSW, STAC):
  basic authentication or an API key in a header (#757).
  The connection keeps the method, user name and header name, never the password or key;
  credentials last only for the session. A login is sent only to its connection's origin
  (scheme, host and port), never after a redirect to another origin. Sessions and connections
  have separate authenticated requests and caches. Plain `http://` requires confirmation
  (`allowInsecure`), and `connection.logout` ends the session login. Layers record their
  connection id in the project (`"connection"`, additive), and ask when its connection or
  session login is missing. Remembering service logins is a separate follow-up.
