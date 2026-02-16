# RustCON JS

RustCON JS is a JavaScript library for interacting with Rust servers via WebSockets (Web RCON)

Also works on browsers and Deno!

## Links

- [Website](https://rustcon.xyz)
- [Documentation](https://rustcon.xyz/developers)
- [GitHub](https://github.rustcon.xyz/)
- [Support](https://support.rustcon.xyz/)

## Installation

```bash
npm install rustcon-js
yarn add rustcon-js
pnpm add rustcon-js
bun add rustcon-js
```

Also available via [Deno package manager](https://deno.land/x/rustcon).

```typescript
import { RconConnection } from "https://deno.land/x/rustcon/mod.ts";
```

## Example Usage

```js
import { RconConnection } from "rustcon-js";
// import { RconConnection } from "https://deno.land/x/rustcon/mod.ts";

const connection = new RconConnection({

    ip: "127.0.0.1",

    port: 28016, // Optional, default is 28016

    password: "password"

});

connection.on("connected", async () => {

    console.log("Connected to the Rust server!");

    const response = await connection.send("say Hello from RustCON JS!");

    console.log("Response from the Rust server:", response);

    const serverInfo = await connection.server.getInfo();

    console.log("Server info:", serverInfo);

    const playerList = await connection.players.getAll();

    console.log("Player list:", playerList);    

    setTimeout(() => {

        connection.disconnect();

    }, 10000);

});

connection.on("message", (message) => {

    console.log("Message from the Rust server:", message);

});

connection.on("error", (error) => {

    console.error("An error occurred:", error);

});

connection.on("disconnected", () => {

    console.log("Disconnected from the Rust server!");

});

connection.connect();
```

## License

[MIT](/LICENSE)
