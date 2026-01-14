//! Host trait implementations for WASI HTTP interfaces
//!
//! This module implements the bindgen-generated Host traits for the WASI HTTP interfaces.
//! The backing types are defined in types.rs and mapped via bindgen's `with` option.

use crate::bindings::wasi::http::types::{
    self, HeaderError, Host as HttpTypesHost, HostFields, HostFutureIncomingResponse,
    HostFutureTrailers, HostIncomingBody, HostIncomingRequest, HostIncomingResponse,
    HostOutgoingBody, HostOutgoingRequest, HostOutgoingResponse, HostRequestOptions,
    HostResponseOutparam, Method, Scheme,
};
use crate::bindings::wasi::http::outgoing_handler::Host as OutgoingHandlerHost;
use crate::events::HttpEventData;
use crate::types::{
    HostFields as FieldsData, HostFutureIncomingResponse as FutureResponseData,
    HostFutureTrailers as FutureTrailersData, HostIncomingBody as IncomingBodyData,
    HostIncomingRequest as IncomingRequestData, HostIncomingResponse as IncomingResponseData,
    HostOutgoingBody as OutgoingBodyData, HostOutgoingRequest as OutgoingRequestData,
    HostOutgoingResponse as OutgoingResponseData, HostRequestOptions as RequestOptionsData,
    HostResponseOutparam as ResponseOutparamData, WasiErrorCode,
};

use theater::actor::ActorStore;
use theater_handler_io::{InputStream, IoError, OutputStream};
use val_serde::IntoSerializableVal;
use wasmtime::component::Resource;

// ============================================================================
// HostFields implementation
// ============================================================================

impl HostFields for ActorStore
{
    async fn new(&mut self) -> wasmtime::Result<Resource<FieldsData>> {
        let data = FieldsData::new();
        let resource = self.resource_table.lock().unwrap().push(data)?;

        // Record the call for replay
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[constructor]fields",
            ().into_serializable_val(),
            resource.rep().into_serializable_val(),
        );

        Ok(resource)
    }

    async fn from_list(
        &mut self,
        entries: Vec<(String, Vec<u8>)>,
    ) -> wasmtime::Result<Result<Resource<FieldsData>, HeaderError>> {
        for (name, _value) in &entries {
            if name.is_empty() {
                return Ok(Err(HeaderError::InvalidSyntax));
            }
        }
        let data = FieldsData::from_list(entries);
        let resource = self.resource_table.lock().unwrap().push(data)?;
        Ok(Ok(resource))
    }

    async fn get(
        &mut self,
        self_: Resource<FieldsData>,
        name: String,
    ) -> wasmtime::Result<Vec<Vec<u8>>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let data: &FieldsData = table.get(&self_)?;
            data.get(&name)
        };

        // Record the call for replay
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]fields.get",
            val_serde::SerializableVal::Tuple(vec![
                self_.rep().into_serializable_val(),
                name.clone().into_serializable_val(),
            ]),
            result.clone().into_serializable_val(),
        );

        Ok(result)
    }

    async fn has(&mut self, self_: Resource<FieldsData>, name: String) -> wasmtime::Result<bool> {
        let table = self.resource_table.lock().unwrap();
        let data: &FieldsData = table.get(&self_)?;
        Ok(data.has(&name))
    }

    async fn set(
        &mut self,
        self_: Resource<FieldsData>,
        name: String,
        value: Vec<Vec<u8>>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let table = self.resource_table.lock().unwrap();
        let data: &FieldsData = table.get(&self_)?;
        if data.is_immutable() {
            return Ok(Err(HeaderError::Immutable));
        }
        data.set(&name, value);
        Ok(Ok(()))
    }

    async fn delete(
        &mut self,
        self_: Resource<FieldsData>,
        name: String,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let table = self.resource_table.lock().unwrap();
        let data: &FieldsData = table.get(&self_)?;
        if data.is_immutable() {
            return Ok(Err(HeaderError::Immutable));
        }
        data.delete(&name);
        Ok(Ok(()))
    }

    async fn append(
        &mut self,
        self_: Resource<FieldsData>,
        name: String,
        value: Vec<u8>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        // Record the call for replay (use bool for success to avoid HeaderError serialization)
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]fields.append",
            val_serde::SerializableVal::Tuple(vec![
                self_.rep().into_serializable_val(),
                name.clone().into_serializable_val(),
                value.clone().into_serializable_val(),
            ]),
            true.into_serializable_val(),
        );

        let table = self.resource_table.lock().unwrap();
        let data: &FieldsData = table.get(&self_)?;
        if data.is_immutable() {
            return Ok(Err(HeaderError::Immutable));
        }
        data.append(&name, value);
        Ok(Ok(()))
    }

    async fn entries(
        &mut self,
        self_: Resource<FieldsData>,
    ) -> wasmtime::Result<Vec<(String, Vec<u8>)>> {
        let table = self.resource_table.lock().unwrap();
        let data: &FieldsData = table.get(&self_)?;
        Ok(data.entries())
    }

    async fn clone(&mut self, self_: Resource<FieldsData>) -> wasmtime::Result<Resource<FieldsData>> {
        let cloned = {
            let table = self.resource_table.lock().unwrap();
            let data: &FieldsData = table.get(&self_)?;
            data.clone_fields()
        };
        let resource = self.resource_table.lock().unwrap().push(cloned)?;
        Ok(resource)
    }

    async fn drop(&mut self, rep: Resource<FieldsData>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostOutgoingRequest implementation
// ============================================================================

impl HostOutgoingRequest for ActorStore
{
    async fn new(
        &mut self,
        headers: Resource<FieldsData>,
    ) -> wasmtime::Result<Resource<OutgoingRequestData>> {
        // Clone the headers data for the request
        let headers_data = {
            let table = self.resource_table.lock().unwrap();
            let h: &FieldsData = table.get(&headers)?;
            h.clone_fields()
        };

        let data = OutgoingRequestData::new(headers_data);
        let resource = self.resource_table.lock().unwrap().push(data)?;
        Ok(resource)
    }

    async fn body(
        &mut self,
        self_: Resource<OutgoingRequestData>,
    ) -> wasmtime::Result<Result<Resource<OutgoingBodyData>, ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut OutgoingRequestData = table.get_mut(&self_)?;
        if request.body.is_some() {
            return Ok(Err(()));
        }
        let body = OutgoingBodyData::new();
        drop(table);

        let resource = self.resource_table.lock().unwrap().push(body)?;
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut OutgoingRequestData = table.get_mut(&self_)?;
        request.body = Some(OutgoingBodyData::new()); // Store a copy
        Ok(Ok(resource))
    }

    async fn method(
        &mut self,
        self_: Resource<OutgoingRequestData>,
    ) -> wasmtime::Result<Method> {
        let table = self.resource_table.lock().unwrap();
        let request: &OutgoingRequestData = table.get(&self_)?;
        // Convert our internal Method to bindgen Method
        Ok(match &request.method {
            crate::types::Method::Get => Method::Get,
            crate::types::Method::Head => Method::Head,
            crate::types::Method::Post => Method::Post,
            crate::types::Method::Put => Method::Put,
            crate::types::Method::Delete => Method::Delete,
            crate::types::Method::Connect => Method::Connect,
            crate::types::Method::Options => Method::Options,
            crate::types::Method::Trace => Method::Trace,
            crate::types::Method::Patch => Method::Patch,
            crate::types::Method::Other(s) => Method::Other(s.clone()),
        })
    }

    async fn set_method(
        &mut self,
        self_: Resource<OutgoingRequestData>,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut OutgoingRequestData = table.get_mut(&self_)?;
        request.method = match method {
            Method::Get => crate::types::Method::Get,
            Method::Head => crate::types::Method::Head,
            Method::Post => crate::types::Method::Post,
            Method::Put => crate::types::Method::Put,
            Method::Delete => crate::types::Method::Delete,
            Method::Connect => crate::types::Method::Connect,
            Method::Options => crate::types::Method::Options,
            Method::Trace => crate::types::Method::Trace,
            Method::Patch => crate::types::Method::Patch,
            Method::Other(s) => crate::types::Method::Other(s),
        };
        Ok(Ok(()))
    }

    async fn path_with_query(
        &mut self,
        self_: Resource<OutgoingRequestData>,
    ) -> wasmtime::Result<Option<String>> {
        let table = self.resource_table.lock().unwrap();
        let request: &OutgoingRequestData = table.get(&self_)?;
        Ok(request.path_with_query.clone())
    }

    async fn set_path_with_query(
        &mut self,
        self_: Resource<OutgoingRequestData>,
        path: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut OutgoingRequestData = table.get_mut(&self_)?;
        request.path_with_query = path;
        Ok(Ok(()))
    }

    async fn scheme(
        &mut self,
        self_: Resource<OutgoingRequestData>,
    ) -> wasmtime::Result<Option<Scheme>> {
        let table = self.resource_table.lock().unwrap();
        let request: &OutgoingRequestData = table.get(&self_)?;
        Ok(request.scheme.as_ref().map(|s| match s {
            crate::types::Scheme::Http => Scheme::Http,
            crate::types::Scheme::Https => Scheme::Https,
            crate::types::Scheme::Other(o) => Scheme::Other(o.clone()),
        }))
    }

    async fn set_scheme(
        &mut self,
        self_: Resource<OutgoingRequestData>,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut OutgoingRequestData = table.get_mut(&self_)?;
        request.scheme = scheme.map(|s| match s {
            Scheme::Http => crate::types::Scheme::Http,
            Scheme::Https => crate::types::Scheme::Https,
            Scheme::Other(o) => crate::types::Scheme::Other(o),
        });
        Ok(Ok(()))
    }

    async fn authority(
        &mut self,
        self_: Resource<OutgoingRequestData>,
    ) -> wasmtime::Result<Option<String>> {
        let table = self.resource_table.lock().unwrap();
        let request: &OutgoingRequestData = table.get(&self_)?;
        Ok(request.authority.clone())
    }

    async fn set_authority(
        &mut self,
        self_: Resource<OutgoingRequestData>,
        authority: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut OutgoingRequestData = table.get_mut(&self_)?;
        request.authority = authority;
        Ok(Ok(()))
    }

    async fn headers(
        &mut self,
        self_: Resource<OutgoingRequestData>,
    ) -> wasmtime::Result<Resource<FieldsData>> {
        // Return a resource to the headers
        let headers_clone = {
            let table = self.resource_table.lock().unwrap();
            let request: &OutgoingRequestData = table.get(&self_)?;
            request.headers.clone_fields()
        };
        // Make the headers immutable when accessed this way
        let immutable_headers = FieldsData::immutable(headers_clone.entries());
        let resource = self.resource_table.lock().unwrap().push(immutable_headers)?;
        Ok(resource)
    }

    async fn drop(&mut self, rep: Resource<OutgoingRequestData>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostOutgoingResponse implementation
// ============================================================================

impl HostOutgoingResponse for ActorStore
{
    async fn new(
        &mut self,
        headers: Resource<FieldsData>,
    ) -> wasmtime::Result<Resource<OutgoingResponseData>> {
        let headers_data = {
            let table = self.resource_table.lock().unwrap();
            let h: &FieldsData = table.get(&headers)?;
            h.clone_fields()
        };

        let data = OutgoingResponseData::new(headers_data);
        let resource = self.resource_table.lock().unwrap().push(data)?;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[constructor]outgoing-response",
            headers.rep().into_serializable_val(),
            resource.rep().into_serializable_val(),
        );

        Ok(resource)
    }

    async fn status_code(
        &mut self,
        self_: Resource<OutgoingResponseData>,
    ) -> wasmtime::Result<types::StatusCode> {
        let table = self.resource_table.lock().unwrap();
        let response: &OutgoingResponseData = table.get(&self_)?;
        let result = response.status;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]outgoing-response.status-code",
            self_.rep().into_serializable_val(),
            result.into_serializable_val(),
        );

        Ok(result)
    }

    async fn set_status_code(
        &mut self,
        self_: Resource<OutgoingResponseData>,
        status: types::StatusCode,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let response: &mut OutgoingResponseData = table.get_mut(&self_)?;
        response.status = status;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]outgoing-response.set-status-code",
            val_serde::SerializableVal::Tuple(vec![
                self_.rep().into_serializable_val(),
                status.into_serializable_val(),
            ]),
            true.into_serializable_val(),
        );

        Ok(Ok(()))
    }

    async fn headers(
        &mut self,
        self_: Resource<OutgoingResponseData>,
    ) -> wasmtime::Result<Resource<FieldsData>> {
        let headers_clone = {
            let table = self.resource_table.lock().unwrap();
            let response: &OutgoingResponseData = table.get(&self_)?;
            response.headers.clone_fields()
        };
        let immutable_headers = FieldsData::immutable(headers_clone.entries());
        let resource = self.resource_table.lock().unwrap().push(immutable_headers)?;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]outgoing-response.headers",
            self_.rep().into_serializable_val(),
            resource.rep().into_serializable_val(),
        );

        Ok(resource)
    }

    async fn body(
        &mut self,
        self_: Resource<OutgoingResponseData>,
    ) -> wasmtime::Result<Result<Resource<OutgoingBodyData>, ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let response: &mut OutgoingResponseData = table.get_mut(&self_)?;
        if response.body.is_some() {
            return Ok(Err(()));
        }
        // Create a single body and clone it - the clone shares the underlying buffer
        // because OutputStream uses Arc<Mutex<>>
        let body = OutgoingBodyData::new();
        let body_clone = body.clone();
        response.body = Some(body_clone);
        drop(table);

        let resource = self.resource_table.lock().unwrap().push(body)?;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]outgoing-response.body",
            self_.rep().into_serializable_val(),
            resource.rep().into_serializable_val(),
        );

        Ok(Ok(resource))
    }

    async fn drop(&mut self, rep: Resource<OutgoingResponseData>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[resource-drop]outgoing-response",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostOutgoingBody implementation
// ============================================================================

impl HostOutgoingBody for ActorStore
{
    async fn write(
        &mut self,
        self_: Resource<OutgoingBodyData>,
    ) -> wasmtime::Result<Result<Resource<OutputStream>, ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let body: &mut OutgoingBodyData = table.get_mut(&self_)?;
        match body.take_stream() {
            Some(stream) => {
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(stream)?;

                self.record_host_function_call(
                    "wasi:http/types@0.2.0",
                    "[method]outgoing-body.write",
                    self_.rep().into_serializable_val(),
                    Some(resource.rep()).into_serializable_val(),
                );

                Ok(Ok(resource))
            }
            None => {
                self.record_host_function_call(
                    "wasi:http/types@0.2.0",
                    "[method]outgoing-body.write",
                    self_.rep().into_serializable_val(),
                    None::<u32>.into_serializable_val(),
                );
                Ok(Err(()))
            }
        }
    }

    async fn finish(
        &mut self,
        this: Resource<OutgoingBodyData>,
        trailers: Option<Resource<FieldsData>>,
    ) -> wasmtime::Result<Result<(), types::ErrorCode>> {
        let trailers_rep = trailers.as_ref().map(|t| t.rep());

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[static]outgoing-body.finish",
            val_serde::SerializableVal::Tuple(vec![
                this.rep().into_serializable_val(),
                trailers_rep.into_serializable_val(),
            ]),
            true.into_serializable_val(),
        );

        self.resource_table.lock().unwrap().delete(this)?;
        Ok(Ok(()))
    }

    async fn drop(&mut self, rep: Resource<OutgoingBodyData>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[resource-drop]outgoing-body",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostIncomingRequest implementation
// ============================================================================

impl HostIncomingRequest for ActorStore
{
    async fn method(&mut self, self_: Resource<IncomingRequestData>) -> wasmtime::Result<Method> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let request: &IncomingRequestData = table.get(&self_)?;
            match &request.method {
                crate::types::Method::Get => Method::Get,
                crate::types::Method::Head => Method::Head,
                crate::types::Method::Post => Method::Post,
                crate::types::Method::Put => Method::Put,
                crate::types::Method::Delete => Method::Delete,
                crate::types::Method::Connect => Method::Connect,
                crate::types::Method::Options => Method::Options,
                crate::types::Method::Trace => Method::Trace,
                crate::types::Method::Patch => Method::Patch,
                crate::types::Method::Other(s) => Method::Other(s.clone()),
            }
        };

        // Record with method as string for serialization
        let method_str = format!("{:?}", result);
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-request.method",
            self_.rep().into_serializable_val(),
            method_str.into_serializable_val(),
        );

        Ok(result)
    }

    async fn path_with_query(
        &mut self,
        self_: Resource<IncomingRequestData>,
    ) -> wasmtime::Result<Option<String>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let request: &IncomingRequestData = table.get(&self_)?;
            request.path_with_query.clone()
        };

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-request.path-with-query",
            self_.rep().into_serializable_val(),
            result.clone().into_serializable_val(),
        );

        Ok(result)
    }

    async fn scheme(
        &mut self,
        self_: Resource<IncomingRequestData>,
    ) -> wasmtime::Result<Option<Scheme>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let request: &IncomingRequestData = table.get(&self_)?;
            request.scheme.as_ref().map(|s| match s {
                crate::types::Scheme::Http => Scheme::Http,
                crate::types::Scheme::Https => Scheme::Https,
                crate::types::Scheme::Other(o) => Scheme::Other(o.clone()),
            })
        };

        // Record with scheme as string for serialization
        let scheme_str = result.as_ref().map(|s| format!("{:?}", s));
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-request.scheme",
            self_.rep().into_serializable_val(),
            scheme_str.into_serializable_val(),
        );

        Ok(result)
    }

    async fn authority(
        &mut self,
        self_: Resource<IncomingRequestData>,
    ) -> wasmtime::Result<Option<String>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let request: &IncomingRequestData = table.get(&self_)?;
            request.authority.clone()
        };

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-request.authority",
            self_.rep().into_serializable_val(),
            result.clone().into_serializable_val(),
        );

        Ok(result)
    }

    async fn headers(
        &mut self,
        self_: Resource<IncomingRequestData>,
    ) -> wasmtime::Result<Resource<FieldsData>> {
        let headers_clone = {
            let table = self.resource_table.lock().unwrap();
            let request: &IncomingRequestData = table.get(&self_)?;
            request.headers.clone_fields()
        };
        let immutable_headers = FieldsData::immutable(headers_clone.entries());
        let resource = self.resource_table.lock().unwrap().push(immutable_headers)?;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-request.headers",
            self_.rep().into_serializable_val(),
            resource.rep().into_serializable_val(),
        );

        Ok(resource)
    }

    async fn consume(
        &mut self,
        self_: Resource<IncomingRequestData>,
    ) -> wasmtime::Result<Result<Resource<IncomingBodyData>, ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let request: &mut IncomingRequestData = table.get_mut(&self_)?;
        let result = match request.body.take() {
            Some(body) => {
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(body)?;
                Ok(resource)
            }
            None => Err(()),
        };

        // Record with resource rep or error (as Option for serialization)
        let output: Option<u32> = result.as_ref().map(|r| r.rep()).ok();
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-request.consume",
            self_.rep().into_serializable_val(),
            output.into_serializable_val(),
        );

        Ok(result)
    }

    async fn drop(&mut self, rep: Resource<IncomingRequestData>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[resource-drop]incoming-request",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );

        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostIncomingResponse implementation
// ============================================================================

impl HostIncomingResponse for ActorStore
{
    async fn status(
        &mut self,
        self_: Resource<IncomingResponseData>,
    ) -> wasmtime::Result<types::StatusCode> {
        let table = self.resource_table.lock().unwrap();
        let response: &IncomingResponseData = table.get(&self_)?;
        Ok(response.status)
    }

    async fn headers(
        &mut self,
        self_: Resource<IncomingResponseData>,
    ) -> wasmtime::Result<Resource<FieldsData>> {
        let headers_clone = {
            let table = self.resource_table.lock().unwrap();
            let response: &IncomingResponseData = table.get(&self_)?;
            response.headers.clone_fields()
        };
        let immutable_headers = FieldsData::immutable(headers_clone.entries());
        let resource = self.resource_table.lock().unwrap().push(immutable_headers)?;
        Ok(resource)
    }

    async fn consume(
        &mut self,
        self_: Resource<IncomingResponseData>,
    ) -> wasmtime::Result<Result<Resource<IncomingBodyData>, ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let response: &mut IncomingResponseData = table.get_mut(&self_)?;
        match response.body.take() {
            Some(body) => {
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(body)?;
                Ok(Ok(resource))
            }
            None => Ok(Err(())),
        }
    }

    async fn drop(&mut self, rep: Resource<IncomingResponseData>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostIncomingBody implementation
// ============================================================================

impl HostIncomingBody for ActorStore
{
    async fn stream(
        &mut self,
        self_: Resource<IncomingBodyData>,
    ) -> wasmtime::Result<Result<Resource<InputStream>, ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let body: &mut IncomingBodyData = table.get_mut(&self_)?;
        let result = match body.take_stream() {
            Some(stream) => {
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(stream)?;
                Ok(resource)
            }
            None => Err(()),
        };

        let output: Option<u32> = result.as_ref().map(|r| r.rep()).ok();
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[method]incoming-body.stream",
            self_.rep().into_serializable_val(),
            output.into_serializable_val(),
        );

        Ok(result)
    }

    async fn finish(
        &mut self,
        this: Resource<IncomingBodyData>,
    ) -> wasmtime::Result<Resource<FutureTrailersData>> {
        let this_rep = this.rep();
        self.resource_table.lock().unwrap().delete(this)?;
        let trailers = FutureTrailersData {
            trailers: Some(Ok(None)),
        };
        let resource = self.resource_table.lock().unwrap().push(trailers)?;

        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[static]incoming-body.finish",
            this_rep.into_serializable_val(),
            resource.rep().into_serializable_val(),
        );

        Ok(resource)
    }

    async fn drop(&mut self, rep: Resource<IncomingBodyData>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[resource-drop]incoming-body",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostResponseOutparam implementation
// ============================================================================

impl HostResponseOutparam for ActorStore
{
    async fn set(
        &mut self,
        param: Resource<ResponseOutparamData>,
        response: Result<Resource<OutgoingResponseData>, types::ErrorCode>,
    ) -> wasmtime::Result<()> {
        // Record the call for replay - use resource rep for Ok, error string for Err
        let response_info: Result<u32, String> = match &response {
            Ok(r) => Ok(r.rep()),
            Err(e) => Err(format!("{:?}", e)),
        };
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[static]response-outparam.set",
            val_serde::SerializableVal::Tuple(vec![
                param.rep().into_serializable_val(),
                response_info.clone().into_serializable_val(),
            ]),
            ().into_serializable_val(),
        );

        // Get the outparam and extract the sender
        let sender = {
            let mut table = self.resource_table.lock().unwrap();
            let outparam: &mut ResponseOutparamData = table.get_mut(&param)?;
            outparam.sender.take()
        };

        if let Some(sender) = sender {
            // Convert the response
            let result = match response {
                Ok(resp_resource) => {
                    let table = self.resource_table.lock().unwrap();
                    let resp: &OutgoingResponseData = table.get(&resp_resource)?;
                    crate::types::ResponseOutparamResult::Response(OutgoingResponseData {
                        status: resp.status,
                        headers: resp.headers.clone_fields(),
                        body: resp.body.clone(),
                    })
                }
                Err(code) => {
                    // Convert ErrorCode to WasiErrorCode
                    let wasi_code = convert_error_code(code);
                    crate::types::ResponseOutparamResult::Error(wasi_code)
                }
            };
            let _ = sender.send(result);
        }

        self.resource_table.lock().unwrap().delete(param)?;
        Ok(())
    }

    async fn drop(&mut self, rep: Resource<ResponseOutparamData>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:http/types@0.2.0",
            "[resource-drop]response-outparam",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostFutureTrailers implementation
// ============================================================================

impl HostFutureTrailers for ActorStore
{
    async fn subscribe(
        &mut self,
        _self_: Resource<FutureTrailersData>,
    ) -> wasmtime::Result<Resource<crate::bindings::wasi::io::poll::Pollable>> {
        // For now, return a pollable that's always ready
        // TODO: Implement proper pollable handling
        anyhow::bail!("subscribe not yet implemented for future-trailers")
    }

    async fn get(
        &mut self,
        self_: Resource<FutureTrailersData>,
    ) -> wasmtime::Result<
        Option<Result<Result<Option<Resource<FieldsData>>, types::ErrorCode>, ()>>,
    > {
        let table = self.resource_table.lock().unwrap();
        let trailers: &FutureTrailersData = table.get(&self_)?;
        match &trailers.trailers {
            Some(Ok(None)) => Ok(Some(Ok(Ok(None)))),
            Some(Ok(Some(fields))) => {
                // Clone the fields and create a resource
                let fields_clone = fields.clone_fields();
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(fields_clone)?;
                Ok(Some(Ok(Ok(Some(resource)))))
            }
            Some(Err(code)) => {
                let error = convert_wasi_error_code(code);
                Ok(Some(Ok(Err(error))))
            }
            None => Ok(None),
        }
    }

    async fn drop(&mut self, rep: Resource<FutureTrailersData>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostFutureIncomingResponse implementation
// ============================================================================

impl HostFutureIncomingResponse for ActorStore
{
    async fn subscribe(
        &mut self,
        _self_: Resource<FutureResponseData>,
    ) -> wasmtime::Result<Resource<crate::bindings::wasi::io::poll::Pollable>> {
        anyhow::bail!("subscribe not yet implemented for future-incoming-response")
    }

    async fn get(
        &mut self,
        self_: Resource<FutureResponseData>,
    ) -> wasmtime::Result<
        Option<Result<Result<Resource<IncomingResponseData>, types::ErrorCode>, ()>>,
    > {
        let mut table = self.resource_table.lock().unwrap();
        let future: &mut FutureResponseData = table.get_mut(&self_)?;

        // If we already have a result, return it
        if let Some(ref result) = future.response {
            return match result {
                Ok(response) => {
                    let response_clone = IncomingResponseData {
                        status: response.status,
                        headers: response.headers.clone_fields(),
                        body: None,
                    };
                    drop(table);
                    let resource = self.resource_table.lock().unwrap().push(response_clone)?;
                    Ok(Some(Ok(Ok(resource))))
                }
                Err(code) => Ok(Some(Ok(Err(convert_wasi_error_code(code))))),
            };
        }

        // Try to receive
        if let Some(mut receiver) = future.receiver.take() {
            match receiver.try_recv() {
                Ok(result) => {
                    future.response = Some(result.clone());
                    match result {
                        Ok(response) => {
                            drop(table);
                            let resource = self.resource_table.lock().unwrap().push(response)?;
                            Ok(Some(Ok(Ok(resource))))
                        }
                        Err(code) => Ok(Some(Ok(Err(convert_wasi_error_code(&code))))),
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    future.receiver = Some(receiver);
                    Ok(None)
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    let error = types::ErrorCode::InternalError(Some("channel closed".to_string()));
                    Ok(Some(Ok(Err(error))))
                }
            }
        } else {
            Ok(Some(Ok(Err(types::ErrorCode::InternalError(Some(
                "future already consumed".to_string(),
            ))))))
        }
    }

    async fn drop(&mut self, rep: Resource<FutureResponseData>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostRequestOptions implementation
// ============================================================================

impl HostRequestOptions for ActorStore
{
    async fn new(&mut self) -> wasmtime::Result<Resource<RequestOptionsData>> {
        let data = RequestOptionsData::default();
        let resource = self.resource_table.lock().unwrap().push(data)?;
        Ok(resource)
    }

    async fn connect_timeout(
        &mut self,
        self_: Resource<RequestOptionsData>,
    ) -> wasmtime::Result<Option<types::Duration>> {
        let table = self.resource_table.lock().unwrap();
        let options: &RequestOptionsData = table.get(&self_)?;
        Ok(options.connect_timeout)
    }

    async fn set_connect_timeout(
        &mut self,
        self_: Resource<RequestOptionsData>,
        duration: Option<types::Duration>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let options: &mut RequestOptionsData = table.get_mut(&self_)?;
        options.connect_timeout = duration;
        Ok(Ok(()))
    }

    async fn first_byte_timeout(
        &mut self,
        self_: Resource<RequestOptionsData>,
    ) -> wasmtime::Result<Option<types::Duration>> {
        let table = self.resource_table.lock().unwrap();
        let options: &RequestOptionsData = table.get(&self_)?;
        Ok(options.first_byte_timeout)
    }

    async fn set_first_byte_timeout(
        &mut self,
        self_: Resource<RequestOptionsData>,
        duration: Option<types::Duration>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let options: &mut RequestOptionsData = table.get_mut(&self_)?;
        options.first_byte_timeout = duration;
        Ok(Ok(()))
    }

    async fn between_bytes_timeout(
        &mut self,
        self_: Resource<RequestOptionsData>,
    ) -> wasmtime::Result<Option<types::Duration>> {
        let table = self.resource_table.lock().unwrap();
        let options: &RequestOptionsData = table.get(&self_)?;
        Ok(options.between_bytes_timeout)
    }

    async fn set_between_bytes_timeout(
        &mut self,
        self_: Resource<RequestOptionsData>,
        duration: Option<types::Duration>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let mut table = self.resource_table.lock().unwrap();
        let options: &mut RequestOptionsData = table.get_mut(&self_)?;
        options.between_bytes_timeout = duration;
        Ok(Ok(()))
    }

    async fn drop(&mut self, rep: Resource<RequestOptionsData>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HttpTypesHost implementation (top-level functions)
// ============================================================================

impl HttpTypesHost for ActorStore
{
    async fn http_error_code(
        &mut self,
        _err: Resource<IoError>,
    ) -> wasmtime::Result<Option<types::ErrorCode>> {
        Ok(None)
    }
}

// ============================================================================
// OutgoingHandlerHost implementation
// ============================================================================

impl OutgoingHandlerHost for ActorStore
{
    async fn handle(
        &mut self,
        request: Resource<OutgoingRequestData>,
        _options: Option<Resource<RequestOptionsData>>,
    ) -> wasmtime::Result<Result<Resource<FutureResponseData>, types::ErrorCode>> {
        // Extract request details
        let (url, method_str, headers_entries) = {
            let table = self.resource_table.lock().unwrap();
            let req: &OutgoingRequestData = table.get(&request)?;

            let scheme = match &req.scheme {
                Some(crate::types::Scheme::Http) => "http",
                Some(crate::types::Scheme::Https) | None => "https",
                Some(crate::types::Scheme::Other(s)) => s.as_str(),
            };

            let authority = match &req.authority {
                Some(a) => a.clone(),
                None => return Ok(Err(types::ErrorCode::HttpRequestUriInvalid)),
            };

            let path = req.path_with_query.as_deref().unwrap_or("/");
            let url = format!("{}://{}{}", scheme, authority, path);
            let method_str = req.method.as_str().to_string();
            let headers = req.headers.entries();

            (url, method_str, headers)
        };

        // Record event
        self.record_handler_event(
            "wasi:http/outgoing-handler/handle".to_string(),
            HttpEventData::OutgoingRequestCall {
                method: method_str.clone(),
                uri: url.clone(),
            },
            Some(format!("WASI HTTP: {} {}", method_str, url)),
        );

        // Create channel for async response
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Spawn HTTP request
        let url_clone = url.clone();
        let method = method_str.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();

            let request_builder = match method.as_str() {
                "GET" => client.get(&url_clone),
                "POST" => client.post(&url_clone),
                "PUT" => client.put(&url_clone),
                "DELETE" => client.delete(&url_clone),
                "PATCH" => client.patch(&url_clone),
                "HEAD" => client.head(&url_clone),
                "OPTIONS" => client.request(reqwest::Method::OPTIONS, &url_clone),
                "TRACE" => client.request(reqwest::Method::TRACE, &url_clone),
                _ => {
                    let _ = tx.send(Err(WasiErrorCode::HttpRequestMethodInvalid));
                    return;
                }
            };

            // Add headers
            let mut request_builder = request_builder;
            for (name, value) in headers_entries {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_bytes(&value) {
                    request_builder = request_builder.header(&name, header_value);
                }
            }

            match request_builder.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let headers: Vec<(String, Vec<u8>)> = response
                        .headers()
                        .iter()
                        .map(|(n, v)| (n.as_str().to_string(), v.as_bytes().to_vec()))
                        .collect();

                    let body_bytes = response.bytes().await.unwrap_or_default().to_vec();

                    let _ = tx.send(Ok(IncomingResponseData {
                        status,
                        headers: FieldsData::immutable(headers),
                        body: Some(IncomingBodyData::new(body_bytes)),
                    }));
                }
                Err(e) => {
                    let error = if e.is_timeout() {
                        WasiErrorCode::ConnectionTimeout
                    } else if e.is_connect() {
                        WasiErrorCode::ConnectionRefused
                    } else {
                        WasiErrorCode::InternalError(Some(e.to_string()))
                    };
                    let _ = tx.send(Err(error));
                }
            }
        });

        // Create future resource
        let future = FutureResponseData {
            receiver: Some(rx),
            response: None,
        };
        let resource = self.resource_table.lock().unwrap().push(future)?;

        Ok(Ok(resource))
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn convert_error_code(code: types::ErrorCode) -> WasiErrorCode {
    match code {
        types::ErrorCode::DnsTimeout => WasiErrorCode::DnsTimeout,
        types::ErrorCode::ConnectionRefused => WasiErrorCode::ConnectionRefused,
        types::ErrorCode::ConnectionTimeout => WasiErrorCode::ConnectionTimeout,
        types::ErrorCode::HttpRequestUriInvalid => WasiErrorCode::HttpRequestUriInvalid,
        types::ErrorCode::HttpRequestMethodInvalid => WasiErrorCode::HttpRequestMethodInvalid,
        types::ErrorCode::InternalError(msg) => WasiErrorCode::InternalError(msg),
        // Add more conversions as needed
        _ => WasiErrorCode::InternalError(Some("unknown error".to_string())),
    }
}

fn convert_wasi_error_code(code: &WasiErrorCode) -> types::ErrorCode {
    match code {
        WasiErrorCode::DnsTimeout => types::ErrorCode::DnsTimeout,
        WasiErrorCode::ConnectionRefused => types::ErrorCode::ConnectionRefused,
        WasiErrorCode::ConnectionTimeout => types::ErrorCode::ConnectionTimeout,
        WasiErrorCode::HttpRequestUriInvalid => types::ErrorCode::HttpRequestUriInvalid,
        WasiErrorCode::HttpRequestMethodInvalid => types::ErrorCode::HttpRequestMethodInvalid,
        WasiErrorCode::InternalError(msg) => types::ErrorCode::InternalError(msg.clone()),
        // Add more conversions as needed
        _ => types::ErrorCode::InternalError(Some("unknown error".to_string())),
    }
}

// ============================================================================
// IO Host trait implementations
// ============================================================================

use crate::bindings::wasi::io::error::{Host as IoErrorHost, HostError as IoHostError};
use crate::bindings::wasi::io::streams::{
    Host as IoStreamsHost, HostInputStream, HostOutputStream, StreamError,
};

impl IoHostError for ActorStore
{
    async fn to_debug_string(
        &mut self,
        _self_: Resource<IoError>,
    ) -> wasmtime::Result<String> {
        Ok("stream error".to_string())
    }

    async fn drop(&mut self, rep: Resource<IoError>) -> wasmtime::Result<()> {
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

impl IoErrorHost for ActorStore
{
    // No methods required - just resource types
}

impl HostInputStream for ActorStore
{
    async fn read(
        &mut self,
        self_: Resource<InputStream>,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let stream: &InputStream = table.get(&self_)?;
            match stream.read(len) {
                Ok(data) => Ok(data),
                Err(_) => Err("closed"),
            }
        };

        // Record the call for replay (use string error to avoid StreamError serialization)
        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[method]input-stream.read",
            val_serde::SerializableVal::Tuple(vec![
                self_.rep().into_serializable_val(),
                len.into_serializable_val(),
            ]),
            result.clone().into_serializable_val(),
        );

        match result {
            Ok(data) => Ok(Ok(data)),
            Err(_) => Ok(Err(StreamError::Closed)),
        }
    }

    async fn blocking_read(
        &mut self,
        self_: Resource<InputStream>,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        // Same as read for our in-memory streams
        self.read(self_, len).await
    }

    async fn skip(
        &mut self,
        _self_: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    async fn blocking_skip(
        &mut self,
        self_: Resource<InputStream>,
        len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        self.skip(self_, len).await
    }

    async fn subscribe(
        &mut self,
        _self_: Resource<InputStream>,
    ) -> wasmtime::Result<Resource<crate::bindings::wasi::io::poll::Pollable>> {
        anyhow::bail!("subscribe not implemented for input-stream")
    }

    async fn drop(&mut self, rep: Resource<InputStream>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[resource-drop]input-stream",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

impl HostOutputStream for ActorStore
{
    async fn check_write(
        &mut self,
        self_: Resource<OutputStream>,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let stream: &OutputStream = table.get(&self_)?;
            match stream.check_write() {
                Ok(available) => Ok(available),
                Err(_) => Err("closed"),
            }
        };

        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[method]output-stream.check-write",
            self_.rep().into_serializable_val(),
            result.clone().into_serializable_val(),
        );

        match result {
            Ok(available) => Ok(Ok(available)),
            Err(_) => Ok(Err(StreamError::Closed)),
        }
    }

    async fn write(
        &mut self,
        self_: Resource<OutputStream>,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let stream: &OutputStream = table.get(&self_)?;
            match stream.write(&contents) {
                Ok(()) => Ok(()),
                Err(_) => Err("closed"),
            }
        };

        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[method]output-stream.write",
            val_serde::SerializableVal::Tuple(vec![
                self_.rep().into_serializable_val(),
                contents.clone().into_serializable_val(),
            ]),
            result.is_ok().into_serializable_val(),
        );

        match result {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(StreamError::Closed)),
        }
    }

    async fn blocking_write_and_flush(
        &mut self,
        self_: Resource<OutputStream>,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let stream: &OutputStream = table.get(&self_)?;
            match stream.write(&contents) {
                Ok(()) => {
                    let _ = stream.flush();
                    Ok(())
                }
                Err(_) => Err("closed"),
            }
        };

        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[method]output-stream.blocking-write-and-flush",
            val_serde::SerializableVal::Tuple(vec![
                self_.rep().into_serializable_val(),
                contents.clone().into_serializable_val(),
            ]),
            result.is_ok().into_serializable_val(),
        );

        match result {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(StreamError::Closed)),
        }
    }

    async fn flush(
        &mut self,
        self_: Resource<OutputStream>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let result = {
            let table = self.resource_table.lock().unwrap();
            let stream: &OutputStream = table.get(&self_)?;
            match stream.flush() {
                Ok(()) => Ok(()),
                Err(_) => Err("closed"),
            }
        };

        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[method]output-stream.flush",
            self_.rep().into_serializable_val(),
            result.is_ok().into_serializable_val(),
        );

        match result {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(StreamError::Closed)),
        }
    }

    async fn blocking_flush(
        &mut self,
        self_: Resource<OutputStream>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        self.flush(self_).await
    }

    async fn subscribe(
        &mut self,
        _self_: Resource<OutputStream>,
    ) -> wasmtime::Result<Resource<crate::bindings::wasi::io::poll::Pollable>> {
        anyhow::bail!("subscribe not implemented for output-stream")
    }

    async fn write_zeroes(
        &mut self,
        _self_: Resource<OutputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    async fn blocking_write_zeroes_and_flush(
        &mut self,
        _self_: Resource<OutputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    async fn splice(
        &mut self,
        _self_: Resource<OutputStream>,
        _src: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    async fn blocking_splice(
        &mut self,
        _self_: Resource<OutputStream>,
        _src: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    async fn drop(&mut self, rep: Resource<OutputStream>) -> wasmtime::Result<()> {
        self.record_host_function_call(
            "wasi:io/streams@0.2.0",
            "[resource-drop]output-stream",
            rep.rep().into_serializable_val(),
            ().into_serializable_val(),
        );
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

impl IoStreamsHost for ActorStore
{
    // No methods required - just resource types
}
