import { z } from 'zod'

// Every schema takes `z` from here, so this runs before any schema is
// built. zod decides when it builds an object schema whether to compile
// its parser with `new Function`, probing for eval to find out; the policy
// forbids eval, and the probe, though caught, files a violation report
// from every page load. Configured in the entry module instead, it ran
// after the schemas: the bundler evaluates the chunk holding them first.
z.config({ jitless: true })

export { z }
