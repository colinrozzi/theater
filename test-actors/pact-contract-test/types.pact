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
    theater:simple/self {
        log: func(msg: string),
    }
}

exports {
    theater:simple/actor.init: func(config: value) -> result<_, string>,
    theater:simple/actor.get-state: func() -> value,
    theater:todo/actions.add: func(title: string) -> result<todo-item, string>,
    theater:todo/actions.toggle: func(id: u32) -> result<_, string>,
    theater:todo/actions.list: func() -> result<list<todo-item>, string>,
}
