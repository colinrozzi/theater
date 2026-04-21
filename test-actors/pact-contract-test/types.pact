record todo-item {
    id: u32,
    title: string,
    done: bool,
}

record actor-state {
    items: list<todo-item>,
    next-id: u32,
}

imports {
    theater:simple/runtime {
        log: func(msg: string),
    }
}

exports {
    theater:simple/actor.init: func(state: value) -> result<actor-state, string>,
    theater:todo/actions.add: func(state: actor-state, title: string) -> result<tuple<actor-state, todo-item>, string>,
    theater:todo/actions.toggle: func(state: actor-state, id: u32) -> result<tuple<actor-state>, string>,
    theater:todo/actions.list: func(state: actor-state) -> result<tuple<actor-state, list<todo-item>>, string>,
}
