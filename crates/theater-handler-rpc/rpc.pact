// Theater RPC Interface
//
// Direct actor-to-actor function calls with full type safety.

interface rpc {
    @package: string = "theater:simple"

    exports {
        // Call a function on another actor
        // actor-id: target actor ID
        // function: function name to call
        // params: function parameters (as dynamic value)
        // options: call options (as dynamic value, may include timeout-ms)
        // Returns: result<value, string>
        call: func(actor-id: string, function: string, params: value, options: value) -> value

        // Check if an actor exports an interface
        // Returns: result<bool, string>
        implements: func(actor-id: string, interface: string) -> value

        // Get list of actor's exported interfaces
        // Returns: result<list<string>, string>
        exports: func(actor-id: string) -> value

        // Describe a target actor's export/import schema from its STATIC module
        // metadata (no application export is executed). Returns a structured
        // dynamic value (shape in theater-handler-rpc/src/describe.rs): fully-
        // qualified callable names, ordered parameter types, result types, and
        // the referenced named-type table — enough to construct a typed `call`
        // without the actor's source.
        // Returns: result<describe-value, string>  (err string for unknown id)
        describe: func(actor-id: string) -> value
    }
}
