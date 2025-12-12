use sea_orm::ActiveModelTrait;

pub trait ActiveClone<A> where A: ActiveModelTrait {
    fn clone_active_model(&self) -> A;

    fn unchanged_active_model() -> A;
}