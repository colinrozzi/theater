#!/bin/bash

# Script to convert handler event recording calls

convert_handler() {
    local file="$1"
    local old_event_type="$2"
    local new_event_type="$3"

    echo "Converting $file..."

    # First, update the Handler trait method signatures
    sed -i '' 's/fn setup_host_functions(&mut self, actor_component: &mut ActorComponent) -> Result<()>/fn setup_host_functions(\&mut self, actor_component: \&mut ActorComponent<E>) -> Result<()>/' "$file"
    sed -i '' 's/fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()>/fn add_export_functions(\&self, actor_instance: \&mut ActorInstance<E>) -> Result<()>/' "$file"

    # Use perl for multiline replacements
    perl -i -p0e "s/ctx\\.data_mut\\(\\)\\.record_event\\(ChainEventData \\{\\s*event_type: ([^,]+),\\s*data: $old_event_type\\(([^)]+)\\),\\s*timestamp: [^,]+,\\s*description: ([^}]+)\\}\\);/ctx.data_mut().record_handler_event(\$1, $new_event_type::\$2, \$3);/gs" "$file"

    perl -i -p0e "s/actor_component\\.actor_store\\.record_event\\(ChainEventData \\{\\s*event_type: ([^,]+),\\s*data: $old_event_type\\(([^)]+)\\),\\s*timestamp: [^,]+,\\s*description: ([^}]+)\\}\\);/actor_component.actor_store.record_handler_event(\$1, $new_event_type::\$2, \$3);/gs" "$file"

    echo "Done with $file"
}

# Convert each handler
convert_handler "crates/theater-handler-http-framework/src/lib.rs" "EventData::Http" "HttpFrameworkEventData"
convert_handler "crates/theater-handler-http-client/src/lib.rs" "EventData::Http" "HttpEventData"
convert_handler "crates/theater-handler-message-server/src/lib.rs" "EventData::Message" "MessageEventData"
convert_handler "crates/theater-handler-supervisor/src/lib.rs" "EventData::Supervisor" "SupervisorEventData"

echo "All conversions complete!"
