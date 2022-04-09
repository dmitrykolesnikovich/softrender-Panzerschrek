use std::{cell::RefCell, rc::Rc};

pub type CommandArgs = Vec<String>;
pub type CommandHandler<HandlerClass> = fn(&mut HandlerClass, CommandArgs);
pub type NamedCommandHandler<HandlerClass> = (&'static str, CommandHandler<HandlerClass>);

pub trait CommandsQueueInterface
{
	fn has_handler(&self, command: &str) -> bool;
	fn add_invocation(&mut self, command: &str, args: CommandArgs);
}

pub struct CommandsQueue<HandlerClass>
{
	handlers: Vec<NamedCommandHandler<HandlerClass>>,

	// Queue of invocations for each registered command.
	invocations: Vec<Vec<CommandArgs>>,
}

pub type CommandsQueuePtr<HandlerClass> = Rc<RefCell<CommandsQueue<HandlerClass>>>;
pub type CommandsQueueDynPtr = Rc<RefCell<dyn CommandsQueueInterface>>;

impl<HandlerClass> CommandsQueue<HandlerClass>
{
	pub fn new(handlers: Vec<NamedCommandHandler<HandlerClass>>) -> CommandsQueuePtr<HandlerClass>
	{
		let invocations = vec![Vec::new(); handlers.len()];
		Rc::new(RefCell::new(Self { handlers, invocations }))
	}

	pub fn process_commands(&mut self, handler: &mut HandlerClass)
	{
		// TODO - optimize iteration over two vectors.
		for (index, invocations) in self.invocations.iter_mut().enumerate()
		{
			let func = self.handlers[index].1;

			for invocation in invocations.drain(..)
			{
				func(handler, invocation);
			}
		}
	}
}

impl<HandlerClass> CommandsQueueInterface for CommandsQueue<HandlerClass>
{
	fn has_handler(&self, command: &str) -> bool
	{
		for (command_name, _) in &self.handlers
		{
			if command == *command_name
			{
				return true;
			}
		}

		false
	}

	fn add_invocation(&mut self, command: &str, args: CommandArgs)
	{
		// TODO - optimize iteration over two vectors.
		for (index, (command_name, _)) in self.handlers.iter().enumerate()
		{
			if command == *command_name
			{
				self.invocations[index].push(args);
				return;
			}
		}
	}
}
