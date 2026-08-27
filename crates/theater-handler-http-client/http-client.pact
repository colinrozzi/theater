// Theater Outbound HTTP Client Interface
//
// Lets an actor make an outbound HTTP(S) request via the host. Permission-gated:
// an actor may only reach hosts its manifest declares in the handler's allowed_hosts.

interface http-client {
    @package: string = "theater:simple"

    record http-header {
        name: string,
        value: string,
    }

    record http-request {
        method: string,
        url: string,
        headers: list<http-header>,
        body: option<list<u8>>,
    }

    record http-response {
        status: u16,
        headers: list<http-header>,
        body: option<list<u8>>,
    }

    exports {
        request: func(req: http-request) -> result<http-response, string>
    }
}
