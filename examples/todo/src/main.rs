#![feature(proc_macro_hygiene)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate diesel;
#[macro_use] extern crate diesel_migrations;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate rocket_contrib;

mod task;
#[cfg(test)] mod tests;

use rocket::Rocket;
use rocket::fairing::AdHoc;
use rocket::request::{Form, FlashMessage};
use rocket::response::{Flash, Redirect};
use rocket_contrib::{templates::Template, serve::StaticFiles};
use diesel::SqliteConnection;

use crate::task::{Task, Todo};

// This macro from `diesel_migrations` defines an `embedded_migrations` module
// containing a function named `run`. This allows the example to be run and
// tested without any outside setup of the database.
embed_migrations!();

#[database("sqlite_database")]
pub struct DbConn(SqliteConnection);

#[derive(Debug, Serialize)]
struct Context<'a> {
    msg: Option<(&'a str, &'a str)>,
    tasks: Vec<Task>
}

impl<'a> Context<'a> {
    pub fn err(conn: &DbConn, msg: &'a str) -> Context<'a> {
        Context { msg: Some(("error", msg)), tasks: Task::all(conn).unwrap_or_default() }
    }

    pub fn raw(conn: &DbConn, msg: Option<(&'a str, &'a str)>) -> Context<'a> {
        match Task::all(conn) {
            Ok(tasks) => Context { msg, tasks },
            Err(e) => {
                error_!("DB Task::all() error: {}", e);
                Context {
                    msg: Some(("error", "Couldn't access the task database.")),
                    tasks: vec![]
                }
            }
        }
    }
}

#[post("/", data = "<todo_form>")]
fn new(todo_form: Form<Todo>, conn: DbConn) -> Flash<Redirect> {
    let todo = todo_form.into_inner();
    if todo.description.is_empty() {
        Flash::error(Redirect::to("/"), "Description cannot be empty.")
    } else if let Err(e) = Task::insert(todo, &conn) {
        error_!("DB insertion error: {}", e);
        Flash::error(Redirect::to("/"), "Todo could not be inserted due an internal error.")
    } else {
        Flash::success(Redirect::to("/"), "Todo successfully added.")
    }
}

#[put("/<id>")]
fn toggle(id: i32, conn: DbConn) -> Result<Redirect, Template> {
    Task::toggle_with_id(id, &conn)
        .map(|_| Redirect::to("/"))
        .map_err(|e| {
            error_!("DB toggle({}) error: {}", id, e);
            Template::render("index", Context::err(&conn, "Failed to toggle task."))
        })
}

#[delete("/<id>")]
fn delete(id: i32, conn: DbConn) -> Result<Flash<Redirect>, Template> {
    Task::delete_with_id(id, &conn)
        .map(|_| Flash::success(Redirect::to("/"), "Todo was deleted."))
        .map_err(|e| {
            error_!("DB deletion({}) error: {}", id, e);
            Template::render("index", Context::err(&conn, "Failed to delete task."))
        })
}

#[get("/")]
fn index(msg: Option<FlashMessage<'_, '_>>, conn: DbConn) -> Template {
    Template::render("index", match msg {
        Some(ref msg) => Context::raw(&conn, Some((msg.name(), msg.msg()))),
        None => Context::raw(&conn, None),
    })
}

fn run_db_migrations(rocket: Rocket) -> Result<Rocket, Rocket> {
    let conn = DbConn::get_one(&rocket).expect("database connection");
    match embedded_migrations::run(&*conn) {
        Ok(()) => Ok(rocket),
        Err(e) => {
            error!("Failed to run database migrations: {:?}", e);
            Err(rocket)
        }
    }
}

fn rocket() -> Rocket {
    rocket::ignite()
        .attach(DbConn::fairing())
        .attach(AdHoc::on_attach("Database Migrations", run_db_migrations))
        .mount("/", StaticFiles::from("static/"))
        .mount("/", routes![index])
        .mount("/todo", routes![new, toggle, delete])
        .attach(Template::fairing())
}

fn main() {
    let _ = rocket().launch();
}
