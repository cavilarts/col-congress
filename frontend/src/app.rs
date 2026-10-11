use leptos::prelude::*;

#[cfg(feature = "ssr")]
pub fn shell(options: leptos::config::LeptosOptions) -> impl IntoView {
    use leptos::hydration::{AutoReload, HydrationScripts};

    view! {
        <!DOCTYPE html>
        <html lang="es">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <link rel="stylesheet" href="/pkg/frontend.css"/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="min-h-screen flex items-center justify-center bg-slate-100">
            <h1 class="text-3xl font-bold text-slate-800">"Congreso"</h1>
        </main>
    }
}
